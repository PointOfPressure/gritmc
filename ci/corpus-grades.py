#!/usr/bin/env python3
"""What each gameplay check can be made to say, and by what.

# The question, and why the existing answer is not one

`ci/checks/gameplay-falsifiable.toml` reads 108 of 108 and has for some time.
It is produced by `ci/corpus-falsifiable.py --sweep`, which mutates a
**recording** and asks whether the comparator noticed. That is a real question
and the ledger is real evidence — about the comparator. Every recording has
bytes at every step, so mutating those bytes moves the verdict of very nearly
anything, and the sweep's own module docstring says so: "axis A finds no live
defect in the corpus today. Its value is the ratchet."

A ratchet is worth having. It is not an answer to the question the rest of this
tree asks, which is **what recorded difference would turn this check red, and
can this server produce it?** A mutated byte is not such a difference, because
no implementation emits it.

# The four verdicts, and why three of them are green today

    GRADES      a wrong implementation moved what this check reads
    VACUOUS     the difference reached the wire and the projection dropped it
    SUBSTRATE   the code ran and the world it ran in could not express it
    UNVISITED   the code the check names was never executed at all

The last three are green in every report anybody has run, and they are three
different findings with three different repairs: widen the projection, change
the rig, or add a step. Calling them all "falsifiable" is what a ledger of
108/108 does.

# Three of these words already mean something else in this tree

`compare::Verdict` has `Vacuous`, `Inert` and `Mute`, and none of them is this
file's `VACUOUS`. They are worth reading before this one, because between them
they already close most of the "measures its own input" shape:

* `Vacuous` — the **reference** said nothing at all.
* `Inert` — the reference spoke and said only what the scenario's own actions
  force (the acknowledgement and the `handleUseItemOn` resyncs at the clicked
  positions), **and the subject said the same**. That is the self-echo
  detector, and it is the reason a check whose only observation is at the
  position its own step touched cannot quietly pass.
* `Mute` — both sides silent at the **measured step** while the scenario as a
  whole is informative. `607-container-click-pickup-all-gathers-matching-
  stacks` was `match` with seven observations a side, every one of them from
  steps 0..5, and its step-6 click produced a `packet:0x60` the check does not
  name. It was green whatever `click.rs` did.

`ci/t2-gameplay.sh` refuses all three and `gameplay-baseline.toml` may not
bless them. **What they have in common is that each asks what the recording
says.** A check can be none of the three and still be unable to go red,
because the question they do not ask is the counterfactual: could a different
implementation have made the recording say something else? A daylight detector
on a superflat reads `power = 0` under any implementation, and a lamp whose
lever is flipped once never runs the code that unlights it. Neither shows up
as vacuous, inert or mute, because both recordings are full of real content
that really did come from the mechanic.

So this file's `VACUOUS` is the narrower claim *the difference existed and this
check's projection dropped it*, which is only decidable against a second
recording from a wrong server. Where the two could be confused the report says
`compare::vacuous` for theirs.

# How each verdict is reached

One corpus recording is taken against a server carrying
`crates/rustmc-server/src/sabotage.rs`'s controls — subsystems replaced with
the not-yet-written versions they grew out of, armed per workspace by
`ci/corpus-controls.py`. Against the clean recording of the same tree:

* the check's report signature **moved** — `GRADES`. This is a positive
  control in the strict sense: a difference a real implementation produces,
  and the check saw it.
* it did not move, and **no armed control ever fired** in that workspace —
  `UNVISITED`. The scenario's steps do not reach the code, so no
  implementation of it could have changed the recording. The absence of a
  `SABOTAGE-FIRST` line is the whole of the evidence and it needs no second
  run.
* it did not move, a control fired, and the **raw recording changed** —
  `VACUOUS`. The server said something different and the check's `observe`
  list or `observe_from` threw it away.
* it did not move, a control fired, and the raw recording is **identical** —
  `SUBSTRATE`. The wrong implementation produced the same bytes as the right
  one, because the world the scenario builds cannot tell them apart. A
  daylight detector on a superflat reads `power = 0` whether or not anything
  recomputes it.

**The raw comparison is what separates the two middle verdicts and it is
deliberately unfiltered** — both recorded clients, every kind, no step
attribution. `observe_from` alone discards a whole recorded connection for
almost every check in the corpus, so a projection-level comparison cannot tell
"the server said nothing different" from "the server said something different
and this check does not listen". Those are the two findings being separated.

# A control armed and never fired is a finding even when the check GRADES

`360-a-lever-lights-a-lamp` flips its lever once. The lamp lights, the check
reads it, and removing `useWithoutItem` reddens it — so it `GRADES`, and
`RedstoneLampBlock`'s four-tick unlight, which is the other half of what a
lamp does, is executed by nothing in that box. A subsystem-level control
cannot see an unvisited branch *inside* a subsystem it otherwise exercises, and
the per-check verdict has no room to say so.

The firing counters do say so, and they say it for every check rather than for
the ones that stayed green. `unexercised_controls` lists the controls armed in
a check's workspace that never reached live code there.

**It is a flag and not a verdict, because one control can suppress another's
code path.** Several are armed in every workspace, and they are not
independent: with `placement` removing a button's pressed state, nothing in
that box ever schedules a tick, so `sched` reports zero firings for a reason
that is about the battery rather than about the scenario. The rehearsal on nine
scenarios showed exactly that — `sched` fired nowhere, and `placement` was
armed in five of the boxes where it would have had something to do.

The `UNVISITED` **verdict** is not exposed to this. It requires that NO armed
control fired at all, and a control that never ran cannot have suppressed
anything: whichever one the scenario reached first would have fired. So the
verdict is safe and the flag is a lead, and the report prints beside each flag
which controls in that same box did fire.

# A check has more than one claim, and the verdict is per check

`710-a-daylight-detector-inverts-when-right-clicked` claims two things: that
the click cycles `inverted`, and that `updateSignalStrength` then recomputes
`power` from the sky. Removing `useWithoutItem` reddens it, so it is `GRADES` —
correctly, and the reading still hides the finding, because the substrate is a
superflat with `advance_time` off and `power` reads 0 on **both** sides
whatever recomputes it. Half of that check's claim cannot be falsified and the
per-check verdict cannot say so.

So a second reading runs beside the first, over the same pair of recordings:
for every block-state property a check's projection carries, does any armed
control move its value? A property that no wrong implementation can shift is a
property the check names and cannot grade, whatever the check's own verdict is.
That reading is `ungraded_properties`, and it is where `710`'s `power` shows up.

**A constant property is not automatically a defect.** `lit = false` on a torch
that is meant to stay dark is a claim worth recording and worth being unable to
move. The output names them and does not refuse them: the finding is that the
check's evidence rests on the properties that did move and on no others.

# What a green under this instrument does not prove

The controls are seven subsystems, not every way an implementation can be
wrong. `GRADES` means *this* difference reddens it, which is a positive
control and not a proof of completeness. The other three verdicts are the
strong ones: they name a difference the check demonstrably cannot see.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Per-trace counters that carry no parity claim on their own. Rebased rather
# than dropped: `compare::rebase_session_counters` does the same thing for the
# same reason, and a raw comparison that failed to would report every scenario
# after the first as changed.
REBASE = {"seq", "window", "state", "entity", "global_index", "index"}

VERDICTS = ("GRADES", "VACUOUS", "SUBSTRATE", "UNVISITED")

LEDGER = ROOT / "ci" / "checks" / "gameplay-grades.toml"

# **The floors are here and not in the ledger, and that is the whole lesson of
# the day this file was written.**
#
# `ci/corpus-falsifiable.py`'s `dead_fields` ratchet read a count out of
# `[meta]` and compared it against `len(dead_field)` from the same file,
# written by the same sweep run — two numbers equal by construction, so the
# refusal could never fire. A bucket count is exactly that kind of number, and
# a ledger that carried its own floor would repeat the defect verbatim.
#
# So `--check` recounts from the ROWS, intersected with the scenario files that
# actually exist, and compares against these. Widening one is a code change
# with a reviewer, which is the same answer `MAX_UNSWEPT` gives.
HISTORY_MOVED_FLOOR = 41      # moved under the reading the gate acts on
# 61 -> 83, 2026-09-24, `recorder-housekeeping` on main 3611. Main's ledger
# was read over 27 recordings and left 42 of stack-gameplay's checks `unseen`
# (never compared). Re-read over the 44 recordings on the box (3e65f5e87,
# Threadripper) every one of them is compared — 26 read frozen, 14 moved, 2
# strict-only — so frozen 61 -> 83 and moved 42 -> 59 (floor 41), with 3
# unresolved (546/547/548, N = 1). The rise is checks that had never been
# asked being asked, not checks that stopped moving: no row main had as
# `moved` reads `frozen` here, and 3 of main's `frozen` now read `moved`.
# 83 -> 98, 2026-09-30, `train-a` (main 3745 + seven lanes, 179 checks), read
# over the box's 46 recordings with the train's own pair (~/rustmc-train-a,
# Threadripper). Of the 98: 57 were `frozen` in main's ledger already, 18 are
# the lanes' new checks now compared twice (their own run and this one), 23
# were `unseen` in main's ledger and are compared for the first time. No row
# main had as `moved` reads `frozen` here; 3 of main's `frozen` read `moved`.
HISTORY_FROZEN_CEILING = 98   # ASKED at least HISTORY_POWER_N times and never moved

# **`frozen` at N=1 is not an agreement, and this ceiling used to count it as
# one. 2026-09-05.**
#
# `ci/corpus-history.py` split `unseen` (N=0) out of `frozen` on 2026-08-18
# because "same word, opposite amount of evidence". It stopped one N short.
# With N recordings there are N-1 opportunities for a signature to differ; at
# N = 1 there are zero, so a check compared once has not been screened, it has
# been counted. That module's report has PRINTED the caveat since the split —
# "of which N rest on a single recording ... surfaced rather than acted on" —
# and this is acting on it.
#
# The redstone branch is what surfaced it: nine new scenarios, every one
# `frozen` at N = 1, breaching this ceiling by 8 while its next-lowest frozen
# row sits at N = 24. Main's frozen population is {18: 13, 19: 1, 61: 2,
# 65: 45}. There is nothing between 1 and 18 in either tree, so the threshold
# separates new checks from screened ones and moves no row on main.
#
# **THE READING THAT WAS PROPOSED AND IS REFUSED HERE: exempting a frozen
# check because it is tier 2 in `ci/checks/gameplay-falsifiable.toml`.**
# That ledger reads tier 2 for **108 of 108** checks, so a ceiling counting
# only frozen-and-below-tier-2 counts ZERO, forever, on every branch — the
# ratchet deleted rather than corrected. This file's opening paragraph is the
# argument against it: the sweep mutates a RECORDING, and "every recording has
# bytes at every step, so mutating those bytes moves the verdict of very
# nearly anything". Tier 2 says the comparator would notice a synthetic byte
# change at the check's own step. It does not say any implementation could
# produce that difference, which is the entire question this file exists to
# ask and the reason `history` is a separate reading at all.
#
# Tier 2 IS the right floor for a check with no history yet, and that is where
# it is used below: an `unresolved` row must carry it, or it has neither kind
# of evidence and is refused by name.
HISTORY_POWER_N = 2

# How many checks may sit unresolved. **14**, which is the largest single
# addition this corpus has on record: `ci/checks/gameplay-baseline.toml`'s
# header, "Corpus 37 -> 51. The fourteen added on 2026-08-03". A number with a
# source rather than a round guess.
#
# It is generous on purpose and it is still a ceiling, because unresolved
# DRAINS in a way frozen does not: the next two corpus recordings take these
# rows to N >= 2, at which point they become `frozen` or `moved` and the
# ceiling above bites. So the failure mode a tighter number would guard
# against — a lane parking fifty ungraded checks here forever — is not stable.
# What this bounds is one branch's worth of new scenarios at a time.
#
# **14 -> 33, 2026-10-01, `train-g`, and it is nine branches' worth.** The train
# lands item-components, train-e, land-batch-1, menus-workstations, fluids,
# mining, commands-exec, random-ticks and train-f in one gate slot, and 33 of
# their new checks have exactly one recording on the box (each lane's own
# subset). `corpus-history.py` over 54 recordings on the train's splice: frozen
# 88, moved 90, strict-only 14, unresolved 33, unseen 135. The gate's own
# recording is the second for every one of them, so this drains on the
# argument above; put it back to 14 when it has.
HISTORY_UNRESOLVED_CEILING = 33

# The positive-control reading. `None` until a run has been taken; `--check`
# refuses a ledger that claims verdicts while this is None, and refuses one
# that carries fewer GRADES than this once it is set.
GRADES_FLOOR = None


def die(msg: str, code: int = 2) -> None:
    print(f"CORPUS-GRADES REFUSED — {msg}", file=sys.stderr)
    sys.exit(code)


def signature(outcomes: list[dict]) -> dict[str, tuple]:
    """Everything the GATED reading says about a scenario, except its prose.

    Richer than the verdict alone for the reason `ci/corpus-falsifiable.py`
    gives: a scenario already at `diverge` stays at `diverge` under almost any
    change, so a verdict is not a change detector.

    **`strict` is deliberately not in here, and leaving it in cost a wrong
    answer.** `ci/selfcheck-aggregate.py` says it plainly — "`strict` is
    reported and **not** gated" — and `ci/t2-gameplay.sh` never mentions it.
    It is the second lens, keeping `observe_from` and dropping the `observe`
    kind list, so it sees differences the check itself declared it was not
    looking at. A check whose `strict` moves and whose declared reading does
    not has NOT been shown falsifiable: nothing would have gone red. It has
    been shown that the corpus recorded the difference and this check does not
    read it, which is the definition of `VACUOUS` and is returned as such.
    """
    return {
        o["name"]: (
            o.get("verdict"),
            o.get("subject_obs"), o.get("measured_subject_obs"),
            tuple(o.get("extra_kinds") or ()), tuple(o.get("missing_kinds") or ()),
            o.get("first_difference"),
        )
        for o in outcomes
    }


def strict_of(outcomes: list[dict]) -> dict[str, str | None]:
    """The ungated second lens, kept beside the gated one rather than inside it."""
    return {o["name"]: o.get("strict") for o in outcomes}


def project(binary: Path, checks: Path, trace: Path, out: Path) -> dict[str, list]:
    """One trace's projection, per scenario — what each check actually reads."""
    subprocess.run([str(binary), "project", str(checks), "--trace", str(trace),
                    "--json", str(out)], capture_output=True, text=True)
    if not out.exists():
        die(f"project wrote no report for {trace}")
    return {s["name"]: (s.get("declared") or [])
            for s in json.loads(out.read_text())["scenarios"]}


def run_compare(binary: Path, checks: Path, ref: Path, sub: Path, out: Path) -> list[dict]:
    r = subprocess.run(
        [str(binary), "compare", str(checks), "--reference", str(ref),
         "--subject", str(sub), "--json", str(out)],
        capture_output=True, text=True)
    if not out.exists():
        die(f"compare wrote no report (rc={r.returncode})\n{r.stdout}\n{r.stderr}")
    return json.loads(out.read_text())["outcomes"]


def raw(trace: Path) -> dict[str, list]:
    """Every recorded observation of every scenario, both clients, every kind.

    Wall-clock is dropped and per-trace counters are rebased; nothing else is
    filtered. This is the lens the check does *not* look through, and the
    whole point of computing it.
    """
    doc = json.loads(trace.read_text())
    out = {}
    for s in doc.get("scenarios", []):
        firsts, rows = {}, []
        for o in s.get("obs", []):
            f = dict(o.get("fields") or {})
            for k in list(f):
                if k in REBASE and isinstance(f[k], int):
                    key = "entity" if k == "entity" else (o["kind"], k)
                    firsts.setdefault(key, f[k])
                    f[k] -= firsts[key]
            rows.append((o.get("who"), o.get("step"), o.get("kind"),
                         tuple(sorted((k, json.dumps(v)) for k, v in f.items()))))
        out[s["name"]] = rows
    return out


def lens_rows(rows, kinds, frm):
    """The two lenses between the raw recording and what a check reads.

    `kind` keeps only the kinds the scenario names, from BOTH recorded
    clients. `declared` keeps only the acting one as well. Naming which of the
    two filters swallowed a difference is the difference between "widen
    `observe`" and "this check is watching the wrong connection", and
    `observe_from` alone discards a whole recorded client for almost every
    check in the corpus.
    """
    kind = [r for r in rows if r[2] in kinds]
    # `both` is the DEFAULT, not a rare opt-in: `scenario.rs` declares
    # `From` with `#[default] Both` and six scenarios leave it unstated.
    # Reading the unstated case as `actor` drops a whole recorded client
    # from the lens that is meant to model what the check reads.
    declared = kind if frm == "both" else [r for r in kind if r[0] == frm]
    return kind, declared


def which_filter_hid_it(clean_rows, sab_rows, kinds, frm):
    """`observe`, `observe_from`, or `step` — whichever swallowed the change."""
    ck, cd = lens_rows(clean_rows, kinds, frm)
    sk, sd = lens_rows(sab_rows, kinds, frm)
    if cd != sd:
        return "step attribution or the comparator's own filters"
    if ck != sk:
        return f"`observe_from = {frm}` — the difference is on the other recorded client"
    return "the `observe` kind list — the kinds it names did not change"


def fired_from(log: Path) -> set[tuple[str, int]]:
    """`(control, workspace)` pairs that reached live code.

    A control announces only its FIRST firing per workspace, because a corpus
    run ends by killing the server and a report printed at shutdown is a
    report nobody reads. Absence here is the `UNVISITED` verdict.
    """
    pat = re.compile(r"^SABOTAGE-FIRST (\w+) ws=(\d+)$")
    out = set()
    for line in log.read_text(errors="replace").splitlines():
        m = pat.match(line.strip())
        if m:
            out.add((m.group(1), int(m.group(2))))
    return out


def state_properties(obs):
    """`(block, property) -> {values}` over a projection's block states.

    `section_blocks_update` carries its states in a payload list rather than in
    `fields.state`, and a reading that missed it would call every property of
    every block in a two-chunk update ungraded.
    """
    out = {}
    def take(state):
        if not isinstance(state, str) or "[" not in state:
            return
        block, rest = state.split("[", 1)
        for pair in rest.rstrip("]").split(","):
            if "=" in pair:
                k, v = pair.split("=", 1)
                out.setdefault((block, k), set()).add(v)
    for o in obs:
        f = o.get("fields") or {}
        take(f.get("state"))
        for b in (f.get("blocks") or []):
            take(b.get("state"))
    return out


def ungraded_properties(clean_obs, sab_obs):
    """Properties the check names whose value no armed control could move."""
    a, b = state_properties(clean_obs), state_properties(sab_obs)
    out = []
    for key, vals in sorted(a.items()):
        if b.get(key, set()) == vals and len(vals) == 1:
            out.append(f"{key[0]}.{key[1]}={next(iter(vals))}")
    return out


def classify(name, moved, armed, fired, raw_changed, strict_moved=False):
    if moved:
        return "GRADES", "a wrong implementation moved what this check reads"
    if not fired:
        which = ",".join(sorted(armed)) or "none"
        return "UNVISITED", f"no armed control ({which}) ever reached live code here"
    f = ",".join(sorted(fired))
    if strict_moved:
        return "VACUOUS", (f"{f} moved this scenario's ungated `strict` reading and not its "
                           "declared one — the corpus saw the difference, the check does not")
    if raw_changed:
        return "VACUOUS", f"{f} changed the recording and the projection dropped it"
    return "SUBSTRATE", f"{f} ran and produced the same bytes as the right answer"


def report(binary, checks, ref, clean, sabotaged, controls, log, out_path):
    docs = {}
    for p in sorted(Path(checks).glob("*.toml")):
        d = tomllib.load(open(p, "rb"))
        docs[d["name"]] = (set(d.get("observe") or []), d.get("observe_from", "both"))
    ctl = json.loads(Path(controls).read_text())
    by_ws = {r["ws"]: r for r in ctl["rows"]}
    by_name = {r["name"]: r for r in ctl["rows"]}
    fired = fired_from(Path(log))
    work = out_path.parent
    out_clean = run_compare(binary, checks, ref, clean, work / "_clean.json")
    out_sab = run_compare(binary, checks, ref, sabotaged, work / "_sab.json")
    sig_clean, sig_sab = signature(out_clean), signature(out_sab)
    strict_clean, strict_sab = strict_of(out_clean), strict_of(out_sab)
    raw_clean, raw_sab = raw(clean), raw(sabotaged)
    proj_clean = project(binary, checks, clean, work / "_pclean.json")
    proj_sab = project(binary, checks, sabotaged, work / "_psab.json")

    rows = []
    for name, row in sorted(by_name.items()):
        ws = row["ws"]
        armed = set(row["controls"])
        f = {k for (k, w) in fired if w == ws and k in armed}
        if name not in sig_clean or name not in sig_sab:
            rows.append({"name": name, "ws": ws, "file": row["file"],
                         "verdict": "UNVISITED", "why": "not in one of the recordings",
                         "armed": sorted(armed), "fired": sorted(f)})
            continue
        moved = sig_clean[name] != sig_sab[name]
        rc = raw_clean.get(name) != raw_sab.get(name)
        sm = strict_clean.get(name) != strict_sab.get(name)
        v, why = classify(name, moved, armed, f, rc, sm)
        hidden = None
        if v == "VACUOUS" and name in docs:
            kinds, frm = docs[name]
            hidden = which_filter_hid_it(raw_clean.get(name, []), raw_sab.get(name, []),
                                         kinds, frm)
            why = f"{why}; hidden by {hidden}"
        ung = ungraded_properties(proj_clean.get(name, []), proj_sab.get(name, []))
        unexercised = sorted(armed - f)
        rows.append({"name": name, "ws": ws, "file": row["file"], "verdict": v,
                     "why": why, "armed": sorted(armed), "fired": sorted(f),
                     "raw_changed": rc, "strict_moved": sm,
                     "input_control": row["input_control"],
                     "ungraded_properties": ung, "hidden_by": hidden,
                     "unexercised_controls": unexercised,
                     "clean_verdict": sig_clean[name][0], "sabotaged_verdict": sig_sab[name][0]})
    out_path.write_text(json.dumps({"rows": rows}, indent=1))
    counts = {v: sum(1 for r in rows if r["verdict"] == v) for v in VERDICTS}
    print(f"== corpus     {checks}  ({len(rows)} checks)")
    print(f"== clean      {clean}")
    print(f"== sabotaged  {sabotaged}")
    print("== " + "   ".join(f"{v} {counts[v]}" for v in VERDICTS))
    for v in VERDICTS[1:]:
        named = [r for r in rows if r["verdict"] == v]
        if not named:
            continue
        print(f"\n-- {v} ({len(named)})")
        for r in named:
            print(f"   {r['file'][:58]:60} {r['why']}")
    unex = [r for r in rows if r.get("verdict") == "GRADES" and r.get("unexercised_controls")]
    if unex:
        print(f"\n-- graded, with a subsystem in the box that nothing exercised ({len(unex)})")
        print("   A LEAD, NOT A VERDICT. The check goes red under a real difference and an")
        print("   armed control never reached live code in its workspace — which is either")
        print("   a subsystem the scenario does not visit, or one that another control in")
        print("   the same box stopped it reaching. What did fire is printed beside it, so")
        print("   the second reading can be ruled out by eye.")
        for r in sorted(unex, key=lambda r: r["file"]):
            print(f"   {r['file'][:46]:48} never ran: {', '.join(r['unexercised_controls'])}"
                  f"   (fired: {', '.join(r['fired']) or 'nothing'})")
    ung = [r for r in rows if r.get("ungraded_properties")]
    if ung:
        n = sum(len(r["ungraded_properties"]) for r in ung)
        print(f"\n-- properties named but not gradeable ({n} across {len(ung)} checks)")
        print("   a property no armed control could move. Not automatically a defect —")
        print("   see the module note — but the check's evidence rests elsewhere.")
        for r in sorted(ung, key=lambda r: r["file"]):
            print(f"   {r['file'][:58]:60} {', '.join(r['ungraded_properties'])}")
    return 0


def selftest() -> int:
    """The house pattern: no recordings, no server, no comparator.

    Every arm of `classify` is exercised, because the classification is the
    whole product and a table nobody can see go wrong is the shape this
    instrument exists to refuse.
    """
    cases = [
        ("moved",      dict(moved=True,  armed={"use"}, fired={"use"}, raw_changed=False), "GRADES"),
        ("moved-too",  dict(moved=True,  armed={"use"}, fired=set(),   raw_changed=False), "GRADES"),
        ("never-ran",  dict(moved=False, armed={"sched"}, fired=set(), raw_changed=False), "UNVISITED"),
        ("hidden",     dict(moved=False, armed={"use"}, fired={"use"}, raw_changed=True),  "VACUOUS"),
        ("same-bytes", dict(moved=False, armed={"use"}, fired={"use"}, raw_changed=False), "SUBSTRATE"),
        # The gate acts on the declared reading only. A scenario whose ungated
        # `strict` moved and whose declared reading did not would have reddened
        # nothing, and calling that GRADES was this instrument's own first bug.
        ("strict-only", dict(moved=False, armed={"placement"}, fired={"placement"},
                             raw_changed=True, strict_moved=True), "VACUOUS"),
    ]
    bad = []
    for name, kw, want in cases:
        got, why = classify(name, **kw)
        if got != want:
            bad.append(f"{name}: want {want}, got {got} ({why})")
    # A control that fired but is not armed for this workspace must not count:
    # it is another scenario's evidence and would turn every UNVISITED green.
    got, _ = classify("x", moved=False, armed={"sched"}, fired=set(), raw_changed=True)
    if got != "UNVISITED":
        bad.append(f"an unfired armed control must outrank a raw change, got {got}")
    # The second reading, on the case it was written for. `710`'s projection
    # carries `inverted` and `power`; removing `useWithoutItem` moves the first
    # and cannot move the second, because a superflat with the sun pinned reads
    # `power = 0` whatever recomputes it.
    clean = [{"fields": {"state": "minecraft:daylight_detector[inverted=true,power=0]"}}]
    sab = [{"fields": {"state": "minecraft:daylight_detector[inverted=false,power=0]"}}]
    got = ungraded_properties(clean, sab)
    if got != ["minecraft:daylight_detector.power=0"]:
        bad.append(f"710-shaped ungraded reading: got {got}")
    # A property carried only inside `section_blocks_update`'s payload must be
    # read too: missing it would call every property of every block in a
    # two-chunk update ungraded, which is most of the redstone corpus.
    packed_a = [{"fields": {"blocks": [{"state": "minecraft:redstone_lamp[lit=true]"}]}}]
    packed_b = [{"fields": {"blocks": [{"state": "minecraft:redstone_lamp[lit=false]"}]}}]
    if ungraded_properties(packed_a, packed_b) != []:
        bad.append("a section_blocks_update payload was not read")
    if ungraded_properties(packed_a, packed_a) != ["minecraft:redstone_lamp.lit=true"]:
        bad.append("a constant section_blocks_update property was not reported")

    # Which filter swallowed a difference. Three arms, because the repair is
    # different for each: widen `observe`, watch the other connection, or look
    # at the comparator. `observe_from` alone discards a whole recorded client
    # for almost every check in the corpus, so guessing is not an option.
    row = lambda who, kind, v: (who, 0, kind, (("state", json.dumps(v)),))
    lens_cases = [
        ("the declared lens itself moved",
         [row("actor", "block_update", "a")], [row("actor", "block_update", "b")],
         {"block_update"}, "actor", "step attribution"),
        ("the difference is on the other client",
         [row("observer", "block_update", "a")], [row("observer", "block_update", "b")],
         {"block_update"}, "actor", "observe_from"),
        ("the difference is in a kind it does not name",
         [row("actor", "sound", "a")], [row("actor", "sound", "b")],
         {"block_update"}, "actor", "kind list"),
    ]
    for label, c, sb, kinds, frm, expect in lens_cases:
        got = which_filter_hid_it(c, sb, kinds, frm)
        if expect not in got:
            bad.append(f"{label}: expected {expect!r} in {got!r}")

    # ---- the ledger gate, shown red six ways ----------------------------
    #
    # `--check` is what T0 runs and it needs no recordings, which is exactly
    # the shape that quietly stops being able to fail. Every refusal it
    # advertises is reconstructed here.
    import shutil, tempfile
    work = Path(tempfile.mkdtemp(prefix="corpus-grades-selftest-"))
    cdir = work / "checks"
    cdir.mkdir()
    for i, (nm, obs) in enumerate((("alpha", "block_update"), ("beta", "sound"))):
        (cdir / f"0{i}0-{nm}.toml").write_text(
            f'name = "{nm}"\nabout = "prose that must not move the digest"\n'
            f'observe = ["{obs}"]\n\n[[step]]\ndo = "dig"\nat = [1, -1, 0]\n')

    def ledger_for(rows, path):
        L = ["[meta]", "history_recordings = 65", "positive_control = false", ""]
        for r in rows:
            L.append("[[check]]")
            for k, v in r.items():
                L.append(f'{k} = "{v}"' if isinstance(v, str) else f"{k} = {v}")
            L.append("")
        path.write_text("\n".join(L))
        return path

    scen = load_scenarios(cdir)
    good = [{"name": n, "file": f, "digest": semantic_digest(d), "history": "moved",
             "history_n": 65} for n, (f, d) in sorted(scen.items())]

    # The falsifiability ledger the unresolved arms read, written BESIDE the
    # grades ledger because `_falsifiable_tiers` looks for it there.
    def tiers_for(mapping, path):
        L = []
        for n, t in mapping.items():
            L += ["[[scenario]]", f'name = "{n}"', f'tier = {t}', ""]
        path.write_text("\n".join(L))

    global HISTORY_MOVED_FLOOR, HISTORY_FROZEN_CEILING, GRADES_FLOOR
    global HISTORY_UNRESOLVED_CEILING
    keep = (HISTORY_MOVED_FLOOR, HISTORY_FROZEN_CEILING, GRADES_FLOOR,
            HISTORY_UNRESOLVED_CEILING)
    HISTORY_MOVED_FLOOR, HISTORY_FROZEN_CEILING, GRADES_FLOOR = 2, 0, None
    led = work / "gameplay-grades.toml"
    tier_led = work / "gameplay-falsifiable.toml"
    tiers_for({"alpha": 2, "beta": 2}, tier_led)

    def want(case, ok, detail=""):
        if not ok:
            bad.append(f"{case}{': ' + detail if detail else ''}")

    want("a complete, current ledger passes",
         check_ledger(cdir, ledger_for(good, led)) == 0)
    want("UNREGISTERED refused — a scenario with no row",
         check_ledger(cdir, ledger_for(good[:1], led)) == 1)
    want("ORPHAN refused — a row with no scenario",
         check_ledger(cdir, ledger_for(
             good + [{"name": "gone", "file": "090-gone.toml", "digest": "sha256:0",
                      "history": "moved", "history_n": 1}], led)) == 1)
    want("STALE refused — the scenario changed since its reading",
         check_ledger(cdir, ledger_for(
             [dict(good[0], digest="sha256:0"), good[1]], led)) == 1)
    # **`about` is prose and must NOT invalidate a reading** — the digest is
    # over the parsed document with it removed, and a ledger that went stale on
    # a typo fix is a ledger nobody re-takes.
    f0 = sorted(cdir.glob("*.toml"))[0]
    f0.write_text(f0.read_text().replace("prose that must not move the digest",
                                         "different prose entirely"))
    want("a prose-only edit does not invalidate the reading",
         check_ledger(cdir, ledger_for(good, led)) == 0)
    want("RATCHET refused — fewer moved than the floor",
         check_ledger(cdir, ledger_for(
             [dict(good[0], history="frozen"), good[1]], led)) == 1)
    HISTORY_FROZEN_CEILING = 5
    HISTORY_MOVED_FLOOR = 0
    want("RATCHET refused — more frozen than the ceiling",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen") for r in good], led)) == 0)
    HISTORY_FROZEN_CEILING = 1
    want("RATCHET refused — more frozen than the ceiling (tightened)",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen") for r in good], led)) == 1)
    # **A verdict with no floor pinned is refused rather than believed.** A
    # bucket count nothing ratchets is the `dead_fields` defect wearing a new
    # name, and this is the arm that stops it being reintroduced here.
    HISTORY_FROZEN_CEILING, HISTORY_MOVED_FLOOR = 5, 0
    want("UNPINNED refused — verdicts recorded while GRADES_FLOOR is None",
         check_ledger(cdir, ledger_for(
             [dict(r, verdict="GRADES") for r in good], led)) == 1)
    # ---------------------------------------------------------------------
    # `frozen` AT N=1 IS NOT AN AGREEMENT — 2026-09-05, and the arms that stop
    # this exemption from swallowing the ratchet it narrows.
    # ---------------------------------------------------------------------
    HISTORY_MOVED_FLOOR, HISTORY_FROZEN_CEILING = 0, 0
    HISTORY_UNRESOLVED_CEILING = 5

    # 1. THE ONE THAT MUST STILL BITE. A genuinely inert check: screened many
    #    times, never moved, and tier 1 in the falsifiability ledger so
    #    neither reading says it can go red. It is `frozen`, it is counted,
    #    and the ceiling refuses it. If this arm ever passes, the exemption
    #    below has eaten the ratchet.
    tiers_for({"alpha": 1, "beta": 1}, tier_led)
    want("RATCHET still refused — a check screened 65 times, never moved, tier 1",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen", history_n=65) for r in good], led)) == 1)
    #    and the same at tier 2, because tier 2 is NOT an exemption from the
    #    frozen ceiling. 108 of 108 checks are tier 2, so treating it as one
    #    would set the counted population to zero forever.
    tiers_for({"alpha": 2, "beta": 2}, tier_led)
    want("RATCHET refused at tier 2 as well — tier does not exempt a SCREENED check",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen", history_n=65) for r in good], led)) == 1)

    # 2. The narrowing itself: the same rows at N=1 are `unresolved`, not
    #    counted against the frozen ceiling, and pass.
    want("frozen at N=1 is unresolved, not counted",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen", history_n=1) for r in good], led)) == 0)
    #    N=2 is the floor and is counted — the boundary, on the counted side.
    want("frozen at N=2 IS counted",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen", history_n=2) for r in good], led)) == 1)

    # 3. An unresolved row has no history evidence, so it must carry the other
    #    kind. Tier 1 and a missing row are both refused BY NAME.
    tiers_for({"alpha": 1, "beta": 2}, tier_led)
    want("UNRESOLVED refused — no history AND tier 1",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen", history_n=1) for r in good], led)) == 1)
    tiers_for({"beta": 2}, tier_led)
    want("UNRESOLVED refused — no history and no falsifiability row at all",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen", history_n=1) for r in good], led)) == 1)
    tier_led.unlink()
    want("UNRESOLVED refused — the falsifiability ledger is absent, so it refuses "
         "rather than skipping",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen", history_n=1) for r in good], led)) == 1)
    tiers_for({"alpha": 2, "beta": 2}, tier_led)

    # 4. And unresolved has its own ceiling, so a lane cannot park an
    #    unbounded number of unscreened checks here.
    HISTORY_UNRESOLVED_CEILING = 1
    want("RATCHET refused — more unresolved than the ceiling",
         check_ledger(cdir, ledger_for(
             [dict(r, history="frozen", history_n=1) for r in good], led)) == 1)

    # 5. **The counted state is re-derived from `history_n`, not read off
    #    `history`.** A pre-2026-09-05 ledger says `frozen` at N=1 and is
    #    counted as unresolved with nobody re-taking anything; and a row that
    #    CLAIMS unresolved at N=65 is still counted as frozen, so the word
    #    cannot be used to escape the ceiling.
    HISTORY_UNRESOLVED_CEILING = 5
    want("a row claiming `unresolved` at N=65 does not escape the frozen ceiling",
         check_ledger(cdir, ledger_for(
             [dict(r, history="unresolved", history_n=65) for r in good], led)) == 1)

    HISTORY_MOVED_FLOOR, HISTORY_FROZEN_CEILING, GRADES_FLOOR, \
        HISTORY_UNRESOLVED_CEILING = keep
    shutil.rmtree(work, ignore_errors=True)

    if bad:
        for b in bad:
            print(f"SELFTEST FAILED — {b}", file=sys.stderr)
        return 1
    print(f"corpus-grades selftest: {len(cases) + 1} classify cases, 3 property "
          f"readings, {len(lens_cases)} lens readings and 19 ledger-gate cases "
          "(15 of them refusals), all arms covered")
    return 0


def semantic_digest(doc: dict) -> str:
    """Borrowed from `ci/corpus-falsifiable.py` rather than reimplemented.

    A second copy of a digest is a second thing to be wrong, and the two files
    have to agree about what "this scenario changed" means or a row can be
    stale in one ledger and current in the other.
    """
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "_cf", ROOT / "ci" / "corpus-falsifiable.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.semantic_digest(doc)


def load_scenarios(checks: Path) -> dict:
    out = {}
    for f in sorted(Path(checks).glob("*.toml")):
        doc = tomllib.load(open(f, "rb"))
        out[doc["name"]] = (f.name, doc)
    return out


def write_ledger(checks: Path, history: Path, grades: Path | None, out: Path) -> int:
    """Generate `ci/checks/gameplay-grades.toml`.

    Two readings share one file because they answer the same question with
    different evidence and a reader needs both: the history says whether
    anything ever moved this check, and the positive control says whether a
    wrong implementation can.
    """
    scen = load_scenarios(checks)
    hist = {r["name"]: r for r in json.loads(Path(history).read_text())["rows"]}
    hbuilds = json.loads(Path(history).read_text())["builds"]
    verd = {}
    if grades is not None:
        verd = {r["name"]: r for r in json.loads(Path(grades).read_text())["rows"]}

    L = [
        "# gameplay-grades.toml — what each gameplay check can be made to say.",
        "#",
        "# Generated by `ci/corpus-grades.py --write`; read by `--check` in",
        "# `ci/t0.sh`, which needs no recordings. Do not hand-edit a row: the",
        "# digest ties a reading to the version of the scenario it was taken",
        "# against, and a hand-written verdict is evidence of nothing.",
        "#",
        "# `history` is the cheap reading, over every corpus recording on the",
        "# box: hold the reference fixed, swap the subject across every build,",
        "# and see whether this check's report signature ever moved.",
        "#",
        "#   moved        it moved under the reading the gate acts on",
        "#   strict-only  it moved only under the ungated `strict` lens, so a",
        "#                real difference reached the corpus and this check did",
        "#                not read it — nothing would have gone red",
        "#   frozen       it was compared at least twice and never moved. A",
        "#                SCREEN AND NOT A VERDICT: a check can be frozen",
        "#                because this server has been right about it since",
        "#                before the oldest recording on the box.",
        "#   unresolved   it was compared ONCE, so the screen was never asked —",
        "#                with N recordings there are N-1 opportunities for the",
        "#                signature to differ, and at N=1 there are none. Read",
        "#                as `frozen` until 2026-09-05. An unresolved check must",
        "#                carry tier 2 in gameplay-falsifiable.toml, which is the",
        "#                most that can be known before builds accumulate; it is",
        "#                NOT grounds to stop counting a screened one.",
        "#   unseen       no recording ever carried it (N=0). Split out",
        "#                2026-08-18 for the same reason, one N earlier.",
        "#",
        "# `--check` re-derives frozen/unresolved from `history_n` rather than",
        "# reading the word, in BOTH directions, or the word is an escape hatch.",
        "#",
        "# `verdict` is the expensive reading, against a server carrying",
        "# `crates/rustmc-server/src/sabotage.rs`'s controls — subsystems",
        "# replaced with the not-yet-written versions they grew out of:",
        "#",
        "#   GRADES     a wrong implementation moved what this check reads",
        "#   VACUOUS    the difference reached the wire, the projection dropped it",
        "#   SUBSTRATE  the code ran and the world could not express it",
        "#   UNVISITED  the code the check names was never executed at all",
        "#",
        "# The last three all read as green in every report anybody has run and",
        "# they are three different findings with three different repairs.",
        "#",
        "# **The floors are in ci/corpus-grades.py, not in this file.** A ledger",
        "# carrying its own floor is a ratchet comparing a number to itself, and",
        "# that is the exact defect this instrument was built after finding.",
        "",
        "[meta]",
        f"history_recordings = {hbuilds}",
        f'positive_control = {"true" if verd else "false"}',
        "",
    ]
    counts = {}
    for name, (fn, doc) in sorted(scen.items()):
        h = hist.get(name, {})
        v = verd.get(name, {})
        counts[h.get("state", "unswept")] = counts.get(h.get("state", "unswept"), 0) + 1
        L += [
            "[[check]]",
            f'name = "{name}"',
            f'file = "{fn}"',
            f'digest = "{semantic_digest(doc)}"',
            f'history = "{h.get("state", "unswept")}"',
            f'history_n = {h.get("N", 0)}',
        ]
        if v.get("verdict"):
            L.append(f'verdict = "{v["verdict"]}"')
            if v.get("fired"):
                L.append("fired = [" + ", ".join(f'"{k}"' for k in v["fired"]) + "]")
        L.append("")
    Path(out).write_text("\n".join(L))
    print(f"wrote {out}: {len(scen)} checks, history over {hbuilds} recordings, "
          + ", ".join(f"{k} {n}" for k, n in sorted(counts.items()))
          + (f", {len(verd)} carrying a positive-control verdict" if verd else
             ", no positive-control reading taken"))
    return 0


def _falsifiable_tiers(beside: Path) -> "dict[str, int] | None":
    """name -> tier from `gameplay-falsifiable.toml`, or None.

    Located BESIDE the grades ledger rather than at a path off ROOT: the two
    ledgers describe the same corpus and a gate that read one tree's tiers
    against another tree's history would be comparing two different branches.
    Read rather than imported so that this gate still works when the file is
    absent — it returns None and the caller refuses by name, which is a
    refusal and not a skip.
    """
    f = Path(beside).parent / "gameplay-falsifiable.toml"
    if not f.exists():
        return None
    try:
        led = tomllib.loads(f.read_text())
    except tomllib.TOMLDecodeError:
        return None
    return {r["name"]: int(r.get("tier", 0)) for r in led.get("scenario", [])
            if "name" in r}


def check_ledger(checks: Path, ledger: Path) -> int:
    """Refuse on drift. Needs no recordings — this is what T0 runs."""
    if not Path(ledger).exists():
        die(f"no ledger at {ledger} — run `ci/corpus-grades.py --write`")
    led = tomllib.loads(Path(ledger).read_text())
    scen = load_scenarios(checks)
    rows = {r["name"]: r for r in led.get("check", [])}
    fail = []

    for name, (fn, doc) in scen.items():
        r = rows.get(name)
        if r is None:
            fail.append(f"UNREGISTERED {name} ({fn}) has no row — a new check has to "
                        "state what it can be made to say")
            continue
        if r.get("digest") != semantic_digest(doc):
            fail.append(f"STALE       {name} ({fn}) changed since its reading was taken"
                        + (f", and the row still claims {r['verdict']}"
                           if r.get("verdict") else ""))
    for name, r in rows.items():
        if name not in scen:
            fail.append(f"ORPHAN      row {name} ({r.get('file')}) has no scenario file")

    live = [r for n, r in rows.items() if n in scen]

    # **The counted state is RE-DERIVED from `history_n`, not read off
    # `history`.** Both are written by the same `--write` run, so believing the
    # word while the number contradicts it is a ratchet over a self-consistent
    # pair — the defect this whole file was built after finding, in this file.
    # A ledger taken before 2026-09-05 says `frozen` at N = 1; it is counted
    # here as `unresolved` without anybody re-taking anything, which is the
    # point of deriving rather than trusting.
    def counted(r):
        h = r.get("history")
        # BOTH directions, or the word becomes an escape hatch: a row that
        # CLAIMS `unresolved` while carrying N = 65 would otherwise sidestep
        # the frozen ceiling by asserting it. `moved` and `strict-only` are
        # untouched — those are decided by signatures differing, which N
        # cannot contradict.
        if h in ("frozen", "unresolved"):
            return ("unresolved" if int(r.get("history_n", 0)) < HISTORY_POWER_N
                    else "frozen")
        return h

    moved = sum(1 for r in live if counted(r) == "moved")
    frozen = sum(1 for r in live if counted(r) == "frozen")
    unresolved = [r for r in live if counted(r) == "unresolved"]

    # An unresolved row has no history evidence at all, so it must carry the
    # other kind: tier 2 in `ci/checks/gameplay-falsifiable.toml`, meaning an
    # edit of the step it is named for moves its outcome. That is a weak claim
    # — too weak to excuse a check screened eighteen times and never moved,
    # which is why it does NOT exempt anything from the frozen ceiling — but it
    # is the most that can be known about a check before builds accumulate, and
    # a check with neither reading is a check with nothing behind it.
    if unresolved:
        tiers = _falsifiable_tiers(ledger)
        if tiers is None:
            fail.append("UNRESOLVED  {} check(s) have been compared fewer than {} times and "
                        "ci/checks/gameplay-falsifiable.toml is unreadable, so their tier "
                        "cannot be confirmed".format(len(unresolved), HISTORY_POWER_N))
        else:
            for r in sorted(unresolved, key=lambda r: r["name"]):
                t = tiers.get(r["name"])
                if t is None:
                    fail.append(f"UNRESOLVED  {r['name']} ({r.get('file')}) has been compared "
                                f"{r.get('history_n')} time(s), so the history screen was "
                                f"never asked, and it has no row in "
                                f"ci/checks/gameplay-falsifiable.toml either")
                elif t < 2:
                    fail.append(f"UNRESOLVED  {r['name']} ({r.get('file')}) has been compared "
                                f"{r.get('history_n')} time(s) AND reads tier {t} — neither "
                                f"reading says this check can go red")
        if len(unresolved) > HISTORY_UNRESOLVED_CEILING:
            fail.append(f"RATCHET     {len(unresolved)} checks have been compared fewer than "
                        f"{HISTORY_POWER_N} times, ceiling is {HISTORY_UNRESOLVED_CEILING} "
                        "(HISTORY_UNRESOLVED_CEILING in ci/corpus-grades.py) — take the "
                        "history reading again before adding more")

    if moved < HISTORY_MOVED_FLOOR:
        fail.append(f"RATCHET     {moved} checks have moved under the gated reading, "
                    f"floor is {HISTORY_MOVED_FLOOR} (HISTORY_MOVED_FLOOR in "
                    "ci/corpus-grades.py) — a check stopped being coupled to anything")
    if frozen > HISTORY_FROZEN_CEILING:
        fail.append(f"RATCHET     {frozen} checks were compared at least "
                    f"{HISTORY_POWER_N} times and never moved, ceiling is "
                    f"{HISTORY_FROZEN_CEILING} (HISTORY_FROZEN_CEILING in "
                    "ci/corpus-grades.py) — a check no real build difference has "
                    "ever reddened")

    graded = [r for r in live if r.get("verdict")]
    if graded and GRADES_FLOOR is None:
        fail.append(f"UNPINNED    {len(graded)} rows carry a positive-control verdict and "
                    "GRADES_FLOOR is None in ci/corpus-grades.py — a bucket count "
                    "nothing ratchets is a number that rots")
    elif GRADES_FLOOR is not None:
        n = sum(1 for r in graded if r["verdict"] == "GRADES")
        if n < GRADES_FLOOR:
            fail.append(f"RATCHET     {n} checks GRADE, floor is {GRADES_FLOOR}")

    if fail:
        print("CORPUS-GRADES RED", file=sys.stderr)
        for f in fail:
            print("  " + f, file=sys.stderr)
        print("\n  to re-take the cheap reading (no lock, no server):\n"
              "    ci/corpus-history.py ci/checks/gameplay <vanilla.json> hist.json\n"
              "    ci/corpus-grades.py --write --history hist.json\n"
              "  the positive-control reading needs ci/corpus-positive-control.sh.",
              file=sys.stderr)
        return 1
    buckets = {v: sum(1 for r in graded if r["verdict"] == v) for v in VERDICTS}
    print(f"corpus-grades: {len(live)} checks; history {moved} moved, "
          f"{sum(1 for r in live if counted(r) == 'strict-only')} strict-only, "
          f"{frozen} frozen, {len(unresolved)} unresolved (N<{HISTORY_POWER_N}), "
          f"over {led.get('meta', {}).get('history_recordings')} recordings"
          + ("; positive control " + ", ".join(f"{k} {n}" for k, n in buckets.items())
             if graded else "; no positive-control reading taken"))
    return 0


USAGE = """usage:
  ci/corpus-grades.py --selftest
  ci/corpus-grades.py --check   [--checks DIR] [--ledger FILE]
  ci/corpus-grades.py --write   --history hist.json [--grades grades.json]
  ci/corpus-grades.py --report --checks DIR --reference V.json \\
      --clean C.json --sabotaged S.json --controls ctl.json \\
      --log server.log --out grades.json [--bin PATH]"""


def main(argv: list[str]) -> int:
    args = argv[1:]
    if not args:
        print(USAGE, file=sys.stderr)
        return 2
    if args[0] == "--selftest":
        return selftest()
    opts = {}
    i = 1
    while i < len(args):
        if not args[i].startswith("--") or i + 1 >= len(args):
            die(USAGE)
        opts[args[i][2:]] = args[i + 1]
        i += 2
    if args[0] == "--check":
        return check_ledger(Path(opts.get("checks", ROOT / "ci" / "checks" / "gameplay")),
                            Path(opts.get("ledger", LEDGER)))
    if args[0] == "--write":
        if "history" not in opts:
            die("--write needs --history <history.json> from ci/corpus-history.py")
        return write_ledger(Path(opts.get("checks", ROOT / "ci" / "checks" / "gameplay")),
                            Path(opts["history"]),
                            Path(opts["grades"]) if "grades" in opts else None,
                            Path(opts.get("out", LEDGER)))
    if args[0] != "--report":
        die(USAGE)
    need = ("checks", "reference", "clean", "sabotaged", "controls", "log", "out")
    for k in need:
        if k not in opts:
            die(f"--report needs --{k}")
    binary = Path(opts.get("bin", ROOT / "target" / "release" / "oracle-runner"))
    return report(binary, Path(opts["checks"]), Path(opts["reference"]),
                  Path(opts["clean"]), Path(opts["sabotaged"]),
                  opts["controls"], opts["log"], Path(opts["out"]))


if __name__ == "__main__":
    sys.exit(main(sys.argv))
