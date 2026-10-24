#!/usr/bin/env python3
"""Which step windows are shorter than the reference's own answer?

# The defect this exists to find

`Client::act` returns, for each step kind, how long the recorder keeps
listening before it issues the next step. That duration is the **step
boundary**, and every observation is filed under whichever step was open when
it arrived. If the boundary is drawn closer than the reference's own answer
latency, then which step a packet is attributed to is decided by the phase of
vanilla's tick loop against the recorder's clock -- a coin flip that reads as
a branch regression.

`Step::Hold` returned 60 ms for as long as the corpus existed. It was found by
hand on 2026-08-09, after a branch gated RED on `040` with a matching digest
and a step off by one, and it cost an agent a turn to establish that the
branch was innocent. `Step::Look` returned the same 60 ms and was left alone
because "no scenario has been measured failing on it" --  which was true, and
which is not the same claim as "the window is long enough".
`docs/PARITY-ROADMAP.md` §12.

Both were found by reading code. This finds them by measuring, over
recordings that already exist, and it finds the whole class rather than one
instance at a time.

# What it measures, and why it needs no model of the windows

For each scenario and each `(who, kind)`, the observations of that kind are
lined up **by ordinal** across every recording and their `step` fields are
compared. The window durations are never used to decide anything: a boundary
that vanilla's own jitter crosses shows up as one ordinal being filed under
step k in some runs and step k+1 in others, and no arithmetic about windows
is required to see that.

The window model *is* used, but only to **name** the boundary at fault in the
report. A wrong constant there produces a mislabelled cause, never a false
finding.

# Separating our defect from vanilla's nondeterminism

These are different findings with opposite fixes, and conflating them is how
`ci/checks/gameplay-unstable.toml` became a list nobody could act on. This is
the distinction `Lens::BoundaryBlind` draws, applied to a whole ensemble:

* same count, same payloads, **different `step`** -- the reference said the
  same thing every time and we filed it in different places. **Ours.** A
  wider window fixes it.
* different count, or different payloads -- the reference said different
  things. **Vanilla's own floor**, `ci/corpus-floor.sh`'s subject, and no
  window change touches it.

# Blast radius, which is the part that decides whether to care

An unstable cell can only move a verdict if it is inside a projection.
`declared` keeps `observe_from` and the `observe` kind list; `strict` keeps
`observe_from` and drops the kind list. A cell outside both is a **latent**
fragility: real, measured, and currently unable to fail. Saying which is
which is the difference between a repair that is worth its corpus run and one
that is worth a line in a doc.

# The cohort rule, mechanised

`--windows` declares the windows the ensemble was recorded at. Recordings
taken either side of a window change are two different worlds and comparing
them across it manufactures instability that is nobody's defect. Pass
`--cohort-from <tree>...` to read each recording's own
`tools/oracle-runner/src/client.rs` and refuse when they disagree on the kind
being audited.

# What would make this come back red

Its own positive control: `--kind hold` over an ensemble that spans the
`Step::Hold` repair must flag `stairs-and-furnace-face-apart`. If it does
not, it is not measuring step attribution and nothing else it says is worth
reading.
"""

import argparse
import json
import pathlib
import re
import sys
import tomllib
from collections import defaultdict

# Used only to *name* the boundary at fault and to place `issue(k)`.
#
# **Not hardcoded**, because the whole subject of this script is a constant
# that was wrong for months while every reader assumed it was right. The
# values are read from the tree the script is run in, and a literal here
# would go stale the moment one of them is repaired -- which is precisely
# what this script exists to cause.
DEFAULT_OTHER = 250
DEFAULT_WINDOWS = {}

# `tools/oracle-runner/src/trace.rs`. `project` drops both of these unless the
# scenario's own `observe` list names them, on the strict path as well as the
# declared one.
HEARTBEAT = "move_entity_heartbeat"
ENTITY_POSITION = "entity_position"

# What counts as "the reference answering" a step of each kind.
#
# This is not a guess and it is not read off the recordings -- deriving the
# causal set from the same data being measured would let a window too short
# to contain an answer prove that no answer exists. It is read out of the
# pinned jar's bytecode, clean-room, and each entry carries the reading that
# justifies it. A kind with no entry gets no latency section rather than a
# section built on an assumption.
ANSWER = {
    "look": {
        "kinds": {"move_entity_rot", "rotate_head"},
        "who": {"observer"},
        "why": (
            "ServerEntity.sendChanges emits ClientboundMoveEntityPacket$Rot "
            "and ClientboundRotateHeadPacket, both inside the "
            "`tickCount % updateInterval == 0` gate; EntityType.PLAYER "
            "declares updateInterval(2). ChunkMap$TrackedEntity.updatePlayer "
            "returns early when `player == this.entity`, so the mover is "
            "never in `seenBy` and a pure rotation reaches the actor not at "
            "all -- server-26.2.jar, 2026-08-09"
        ),
    },
}

# NOT modelled: `near(bound, within)`, the spatial bound, which needs the
# `Places` tracker to resolve an observation's position. It can only ever
# drop a cell further, so a cell this script calls latent is latent -- the
# unmodelled filter cannot put one back inside a projection.


def load_scenarios(checks_dir):
    out = {}
    for p in sorted(pathlib.Path(checks_dir).glob("*.toml")):
        try:
            d = tomllib.loads(p.read_text())
        except Exception as e:  # noqa: BLE001
            print(f"skip {p.name}: {e}", file=sys.stderr)
            continue
        if "name" in d:
            out[d["name"]] = d
    return out


def window_from_tree(tree, kind):
    """The window `Client::act` returns for `kind`, read from that tree."""
    f = pathlib.Path(tree) / "tools/oracle-runner/src/client.rs"
    if not f.is_file():
        return None
    src = f.read_text()
    pat = {
        "look": r"Step::Look \{[^}]*\}\s*=>\s*\{",
        "hold": r"Step::Hold \{[^}]*\}\s*=>\s*\{",
    }.get(kind)
    if pat is None:
        return None
    m = re.search(pat, src)
    if not m:
        return None
    tail = src[m.end():]
    m2 = re.search(r"Ok\(Duration::from_millis\((\d+)\)\)", tail)
    return int(m2.group(1)) if m2 else None


def issue_times(steps, windows):
    t = [0]
    for s in steps:
        k = s.get("do")
        if k == "wait":
            w = int(s.get("ms", 0))
        else:
            w = windows.get(k, DEFAULT_OTHER)
        t.append(t[-1] + w)
    return t


def payload(o):
    return json.dumps(o.get("fields", {}), sort_keys=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--checks", default="ci/checks/gameplay")
    ap.add_argument("--kind", help="only report boundaries owned by this step kind")
    ap.add_argument("--windows", default="", help="e.g. look=60,hold=250")
    ap.add_argument("--cohort-from", nargs="*", default=[],
                    help="trees whose client.rs must agree on --kind's window")
    ap.add_argument("--fail-on-projected", action="store_true",
                    help="exit 1 if any boundary-unstable cell is inside a projection")
    ap.add_argument("--selftest", action="store_true",
                    help="pin the classifier on synthetic cases and exit")
    ap.add_argument("recordings", nargs="*")
    a = ap.parse_args()

    if a.selftest:
        return selftest()

    # Read this tree's own constants first, so the report never quotes a
    # window the code has since moved.
    here = pathlib.Path(__file__).resolve().parent.parent
    windows = dict(DEFAULT_WINDOWS)
    for k in ("look", "hold"):
        v = window_from_tree(here, k)
        if v is not None:
            windows[k] = v
    for part in filter(None, a.windows.split(",")):
        k, _, v = part.partition("=")
        windows[k] = int(v)

    # --- cohort check -----------------------------------------------------
    if a.cohort_from and a.kind:
        seen = {}
        for t in a.cohort_from:
            w = window_from_tree(t, a.kind)
            seen.setdefault(w, []).append(pathlib.Path(t).name)
        print(f"cohort for `{a.kind}`:")
        for w, ts in sorted(seen.items(), key=lambda kv: (kv[0] is None, kv[0])):
            print(f"  {a.kind} = {w} ms : {', '.join(ts)}")
        if len(seen) > 1:
            print(f"\nREFUSED  the ensemble spans a change to `{a.kind}`'s window. "
                  f"Recordings either side of it are two different worlds and any "
                  f"instability this reports would be the change, not a defect.")
            return 2
        only = next(iter(seen))
        if only is not None:
            windows[a.kind] = only
        print()
    elif a.kind:
        # The recordings were made by whatever the *recording* trees carried,
        # which need not be what this tree carries now -- and after a repair
        # it certainly is not. Placing `issue(k)` with the wrong window
        # silently shifts every latency in section B.
        print(f"WARNING  no --cohort-from given: placing steps with this "
              f"tree's `{a.kind}` = {windows.get(a.kind, DEFAULT_OTHER)} ms. "
              f"If the recordings were taken at another value, section B's "
              f"latencies are shifted by the difference.\n")

    scen = load_scenarios(a.checks)
    recs = []
    for r in a.recordings:
        try:
            recs.append((pathlib.Path(r).parent.parent.parent.name, json.load(open(r))))
        except Exception as e:  # noqa: BLE001
            print(f"skip {r}: {e}", file=sys.stderr)
    if not recs:
        print("REFUSED  no readable recordings")
        return 2
    # The two sections do not need the same amount of evidence, and refusing
    # the whole run on N = 1 conflated them.
    #
    # Section A compares recordings against each other, so one recording says
    # nothing and it must refuse. Section B measures arrival latency against
    # a window inside a single recording, and one is enough -- which is
    # exactly the reading wanted immediately after a window is repaired,
    # when by definition only one recording exists at the new value. Making
    # that reading wait for a second corpus run would cost ~40 minutes to
    # learn something the first run already contains.
    solo = len(recs) < 2
    if solo:
        print("NOTE  one recording: section A (attribution stability across "
              "runs) needs at least two and is skipped. Section B (arrival "
              "latency against the window) is measured from this one.\n")

    # cell -> tag -> list of (step, payload) by ordinal, and how many
    # recordings hold each scenario at all -- the corpus denominator has
    # moved (86 -> 88), so "absent" must mean the kind was absent and not
    # that the scenario was.
    cells = defaultdict(dict)
    present = defaultdict(int)
    for tag, data in recs:
        for sc in data["scenarios"]:
            name = sc["name"]
            if name not in scen:
                continue
            present[name] += 1
            by = defaultdict(list)
            for o in sc.get("obs", []):
                by[(o["who"], o["kind"])].append((o["step"], payload(o)))
            for key, seq in by.items():
                cells[(name, key[0], key[1])][tag] = seq

    ours, theirs = classify(cells, present)
    n = len(recs)

    d = scen
    print(f"recordings: {n}  ({', '.join(t for t, _ in recs)})")
    print(f"scenarios:  {len(scen)}\n")

    if not solo:
        print("=" * 78)
        print("OUR BOUNDARY -- same count, same payloads, different step "
              "attribution")
        print("=" * 78)
        if not ours:
            print("  none")
    rows = []
    for name, who, kind, ordinal, got in ours:
        sc = d[name]
        stepdefs = sc.get("step", [])
        allsteps = sorted(got)
        # the boundary at fault closes the earliest step this ordinal was
        # ever filed under, so that step's window is the one that is short
        lo = min(allsteps)
        owner = stepdefs[lo].get("do") if lo < len(stepdefs) else "?"
        if a.kind and owner != a.kind:
            continue
        observe = set(sc.get("observe", []))
        # **It defaults to `both`, not `actor`.**
        # `tools/oracle-runner/src/scenario.rs` declares `From` with
        # `#[default] Both` and its field comment says so. Six scenarios
        # leave it unstated and every one of them watches both
        # connections; with `actor` assumed, every observer-side cell in
        # those six computed `in_from = False` and printed as "latent
        # (outside both projections)" — a fragility declared unable to
        # fail. Under-reporting blast radius is the one direction this
        # column must not err in, because it is what decides whether a
        # repair is worth a corpus run.
        frm = sc.get("observe_from", "both")
        in_from = frm == "both" or frm == who
        # `project` drops these two unless the scenario names them in its own
        # `observe` list, and it reads `observe` for that test even on the
        # strict path -- so an unnamed heartbeat is outside **both**
        # denominators, not just the declared one. Modelling `observe_from`
        # alone overstates the blast radius, which is how the first run of
        # this script labelled three latent cells `strict only`.
        if kind in (HEARTBEAT, ENTITY_POSITION) and kind not in observe:
            in_declared = in_strict = False
        else:
            in_declared = in_from and (not observe or kind in observe)
            in_strict = in_from
        rows.append((name, who, kind, ordinal, lo, owner, allsteps,
                     in_declared, in_strict, got))

    for name, who, kind, ordinal, lo, owner, allsteps, ind, ins, got in rows:
        spread = "  ".join(f"step {s} x{len(got[s])}" for s in sorted(got))
        radius = ("DECLARED+STRICT -- can decide a verdict" if ind else
                  "strict only -- can decide a verdict" if ins else
                  "latent (outside both projections)")
        print(f"\n  {name}")
        print(f"    {who}/{kind} #{ordinal}: filed under {sorted(allsteps)}"
              f"   [{spread}]")
        print(f"    boundary closing step {lo} (`{owner}`, window "
              f"{windows.get(owner, DEFAULT_OTHER)} ms) -- {radius}")

    # ------------------------------------------------------------------
    # Section B exists because section A is blind to the worse case.
    #
    # A stability test can only see a boundary that vanilla's jitter
    # sometimes crosses. A boundary drawn so close that **every** run
    # crosses it is perfectly stable and perfectly wrong: the answer to a
    # step is filed under the following step in all N recordings and no
    # comparison of the recordings against each other will say so. That is
    # this instrument's own version of the `vacuous` verdict, and the two
    # scenarios that exposed it -- `a-bed-facing-north` and `stairs`'
    # second look, whose `rotate_head` is filed one step late in 9 runs out
    # of 9 -- would have been reported clean by section A alone.
    #
    # So the arrival latency is read directly, against the window it had to
    # fit inside. Advisory, not model-free: it needs the window arithmetic
    # to place `issue(k)`, and a step is credited with any observation
    # arriving after it and before one full window past the next boundary.
    if a.kind in ANSWER:
        who_ok, kinds_ok = ANSWER[a.kind]["who"], ANSWER[a.kind]["kinds"]
        print("\n" + "=" * 78)
        print(f"ARRIVAL LATENCY of the reference's answer to a `{a.kind}`, "
              f"against its {windows.get(a.kind, DEFAULT_OTHER)} ms window")
        print("=" * 78)
        print(f"  answer := {sorted(kinds_ok)} on {sorted(who_ok)} "
              f"-- {ANSWER[a.kind]['why']}\n")
        w = windows.get(a.kind, DEFAULT_OTHER)
        lat = defaultdict(list)
        for tag, data in recs:
            for sc in data["scenarios"]:
                if sc["name"] not in scen:
                    continue
                sd = scen[sc["name"]]
                steps = sd.get("step", [])
                iss = issue_times(steps, windows)
                for k, s in enumerate(steps):
                    if s.get("do") != a.kind:
                        continue
                    t0 = iss[k]
                    # up to the next step of the same kind: a second look
                    # supersedes the first, and its answer is its own.
                    nxt = min([iss[j] for j in range(k + 1, len(steps))
                               if steps[j].get("do") == a.kind] + [10 ** 9])
                    seen = set()
                    for o in sc.get("obs", []):
                        if not t0 <= o["at_ms"] < nxt:
                            continue
                        if o["who"] not in who_ok or o["kind"] not in kinds_ok:
                            continue
                        ck = (o["who"], o["kind"])
                        if ck in seen:
                            continue
                        seen.add(ck)
                        lat[(sc["name"], k, ck[0], ck[1])].append(o["at_ms"] - t0)
        if not lat:
            print("  the reference never answers -- check the step actually "
                  "changes something (a look to the yaw the player already "
                  "holds is below the packed-degree deadzone and is not sent)")
    elif a.kind:
        print(f"\n(no answer-set declared for `{a.kind}`, so no latency "
              f"section; see ANSWER in this file)")
    if a.kind in ANSWER:
        late_always = 0
        for (name, k, who, kind), v in sorted(lat.items()):
            v = sorted(v)
            out = sum(1 for x in v if x >= w)
            flag = ""
            if out == len(v):
                flag = "  <== OUTSIDE THE WINDOW IN EVERY RUN"
                late_always += 1
            elif out:
                flag = "  <== crosses the boundary"
            print(f"  {name[:40]:40s} step{k} {who:9s} {kind:22s} "
                  f"n={len(v):2d} min={v[0]:4d} max={v[-1]:4d} "
                  f"late={out}/{len(v)}{flag}")
        print(f"\n  systematically misattributed cells (late in every run): "
              f"{late_always}")

    if not solo:
        print("\n" + "=" * 78)
        print("VANILLA'S OWN FLOOR -- different count or payload; no window "
              "fixes these")
        print("=" * 78)
        if not theirs:
            print("  none")
    for name, who, kind, k, nc, npay in theirs[:30]:
        print(f"  {name:46s} {who:9s} {kind:22s} counts:{nc} payloads:{npay}")
    if len(theirs) > 30:
        print(f"  ... and {len(theirs) - 30} more")

    if solo:
        # Nothing below is a statement this recording can support.
        return 0
    projected = [r for r in rows if r[7] or r[8]]
    print(f"\nboundary-unstable cells owned by "
          f"`{a.kind}`: {len(rows)}" if a.kind else
          f"\nboundary-unstable cells: {len(rows)}")
    print(f"  of which inside a projection (can move a verdict): {len(projected)}")
    print(f"  latent (outside every projection):                 {len(rows) - len(projected)}")
    if a.fail_on_projected and projected:
        print("\nFAIL  a projected verdict is decided by a step boundary")
        return 1
    return 0


def classify(cells, present=None):
    """Split every cell into our boundary, vanilla's floor, or stable.

    `cells` maps `(scenario, who, kind)` to `{tag: [(step, payload), ...]}`.
    `present` maps a scenario to how many recordings contain it at all, so a
    kind that is **absent** from some of them can be told from one that is
    merely late. Returns `(ours, theirs)`; `--selftest` pins the cases.
    """
    ours, theirs = [], []
    for (name, who, kind), per in sorted(cells.items()):
        # A kind seen in some recordings and not others is the reference
        # saying different things, and it is invisible to every test below:
        # `counts` and `pays` are computed over the recordings where the cell
        # exists, so the absences leave no trace in them. Found by injection
        # -- the `len(per) < 2` guard this replaces was unfalsifiable, because
        # a cell from one recording has one step tuple and reads as stable.
        want = present.get(name, len(per)) if present else len(per)
        if len(per) < want:
            theirs.append((name, who, kind, len(per), -1, -1))
            continue
        if len(per) < 2:
            continue
        counts = {len(v) for v in per.values()}
        pays = {tuple(p for _, p in v) for v in per.values()}
        steps = {tuple(s for s, _ in v) for v in per.values()}
        if len(counts) > 1 or len(pays) > 1:
            # The reference said different things. Vanilla's own floor, and no
            # window change touches it -- reported even when the steps also
            # differ, because the step difference cannot be attributed while
            # the payloads are not the same packet.
            #
            # # This is conservative, and it undercounts. Deliberately.
            #
            # The test is per *cell*, not per ordinal, so one missing packet
            # anywhere in a cell demotes the whole cell -- every ordinal in
            # it, including ordinals whose step attribution really is
            # flipping. Measured on the look ensemble: section A reports four
            # unstable rotation cells where the recorded steps show six,
            # because one of the nine recordings is short one
            # `move_entity_rot` and that single absence takes both of
            # `stairs`' looks out of the count.
            #
            # It is left this way because the alternative is worse. Aligning
            # ordinals across runs of unequal length means guessing which
            # packet is missing, and a wrong guess reports a step difference
            # that is really a different packet -- inventing our defect out
            # of vanilla's. Section B has no such coupling and is where the
            # honest per-boundary count comes from; section A's number is a
            # lower bound and should be read as one.
            theirs.append((name, who, kind, len(per), len(counts), len(pays)))
            continue
        if len(steps) == 1:
            continue  # stable
        # Same packets in the same order every run: split per ordinal, so a
        # scenario with two looks reports its two boundaries separately.
        nobs = len(next(iter(per.values())))
        for i in range(nobs):
            got = defaultdict(list)
            for tag, v in per.items():
                got[v[i][0]].append(tag)
            if len(got) > 1:
                ours.append((name, who, kind, i, dict(got)))
    return ours, theirs


def selftest():
    """Pin the classifier on cases whose right answer is not in doubt.

    A gate that cannot fail is not a gate, and an instrument whose own
    classifier is untested is a reading of code wearing a measurement's
    clothes -- which is the exact defect this script exists to find.
    """
    fails = []

    def case(what, got, want):
        if got != want:
            fails.append(f"{what}: got {got}, want {want}")

    P = "payload"
    # 1. Identical everywhere -> stable, reported by neither half.
    ours, theirs = classify({("s", "actor", "k"): {
        "a": [(0, P)], "b": [(0, P)], "c": [(0, P)]}})
    case("stable cell is silent", (len(ours), len(theirs)), (0, 0))

    # 2. Same packet, different step -> OURS. The defect this script is for.
    ours, theirs = classify({("s", "actor", "k"): {
        "a": [(0, P)], "b": [(1, P)]}})
    case("step flip is ours", (len(ours), len(theirs)), (1, 0))
    # Guarded, not indexed blind: a selftest that dies with an IndexError
    # still exits non-zero, but it reports a Python traceback instead of the
    # assertion that failed, and the next reader has to debug the instrument
    # to find out what the instrument was telling them.
    case("flip names its ordinal", ours[0][3] if ours else None, 0)
    case("flip records who was where",
         ours[0][4] if ours else None, {0: ["a"], 1: ["b"]})

    # 3. Different payload -> THEIRS, even though the step also moved.
    #    A step difference cannot be attributed while the packets differ.
    ours, theirs = classify({("s", "actor", "k"): {
        "a": [(0, P)], "b": [(1, "other")]}})
    case("payload change is vanilla's", (len(ours), len(theirs)), (0, 1))

    # 4. Different count -> THEIRS, and the whole cell goes, including the
    #    ordinal that really is flipping. This is the documented undercount;
    #    it is asserted so that "fixing" it cannot happen silently.
    ours, theirs = classify({("s", "actor", "k"): {
        "a": [(0, P), (2, P)], "b": [(1, P)]}})
    case("count change demotes the whole cell",
         (len(ours), len(theirs)), (0, 1))

    # 5. Two ordinals, only one of which moves -> exactly one row.
    ours, theirs = classify({("s", "actor", "k"): {
        "a": [(0, P), (2, P)], "b": [(1, P), (2, P)]}})
    case("only the moving ordinal is reported", len(ours), 1)
    case("and it is ordinal 0", ours[0][3] if ours else None, 0)

    # 6. A single recording can say nothing about stability.
    ours, theirs = classify({("s", "actor", "k"): {"a": [(0, P)]}})
    case("one recording is not an ensemble", (len(ours), len(theirs)), (0, 0))

    # 6b. A kind present in some recordings and absent from others is the
    #     reference saying different things, and nothing in `counts` or
    #     `pays` can see it -- both are computed only over the recordings
    #     where the cell exists. Without `present` this reads as stable.
    ours, theirs = classify(
        {("s", "actor", "k"): {"a": [(0, P)], "b": [(0, P)]}},
        present={"s": 3})
    case("a kind absent from a run is vanilla's",
         (len(ours), len(theirs)), (0, 1))
    ours, theirs = classify(
        {("s", "actor", "k"): {"a": [(0, P)], "b": [(0, P)]}},
        present={"s": 2})
    case("...and is silent when every run has it",
         (len(ours), len(theirs)), (0, 0))

    # 7. The window reader, against this tree's own source.
    here = pathlib.Path(__file__).resolve().parent.parent
    look = window_from_tree(here, "look")
    hold = window_from_tree(here, "hold")
    case("look window is readable", look is not None, True)
    case("hold window is readable", hold is not None, True)
    if look is not None:
        case("a step window is a plausible number of ticks",
             50 <= look <= 2000, True)

    # 8. `issue_times` must accumulate, and `wait` must carry its own ms.
    t = issue_times([{"do": "look"}, {"do": "wait", "ms": 40},
                     {"do": "place"}], {"look": 250})
    case("issue times accumulate", t, [0, 250, 290, 540])

    for f in fails:
        print(f"SELFTEST FAIL  {f}")
    if fails:
        return 1
    print("step-window-audit selftest: 10 cases GREEN")
    return 0



if __name__ == "__main__":
    sys.exit(main())
