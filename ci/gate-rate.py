#!/usr/bin/env python3
"""Measure the green-at-verdict rate p from the gate logs, with its N.

WHY p HAS TO BE MEASURED AND NOT ASSUMED
========================================
`ci/batch.sh`'s whole value is a function of p, and not a gentle one:

    p = 0.60  ->  batching is worth NOTHING; the best width is 1
    p = 0.79  ->  best width 3, ~1.24x serial
    p = 0.90  ->  best width 4, ~1.68x serial
    p = 0.94  ->  best width 8, ~2.21x serial

So a wrong p does not make the plan slightly wrong, it makes it the wrong plan.
`ci/batch-arith.py` carries 0.81 as a default because that is the figure this
work was briefed with; this script is how that figure gets checked, and it is
worth re-running whenever the queue's character changes.

THE INSTRUMENT, AND THE ONE THAT LOOKED EASIER AND IS WRONG
===========================================================
The obvious way to measure p is to grep the orchestrator's own logs for the
verdict lines `ci/handoff.sh` prints — `MERGED`, `RED`, `GREEN`, `INFRA`. Done
over `/home/sayer/grit/*.log` on 2026-08-17 that gives **48 green of 80, p =
0.60**, and it is a biased sample: those logs exist because somebody redirected
a gate they were watching, and people watch gates they expect to fail. **You tee
what you investigate.** A rate taken over the runs somebody chose to keep is not
the rate over the runs.

This script reads instead the log `ci/handoff.sh` writes for **every** gate,
unconditionally, at `$HOME/rustmc-private/gates/gate-<branch>-<stamp>.log`, and
classifies it by handoff's own stage sentinels — the same three strings its own
GREEN condition is built from (`ci/handoff.sh` ~line 1022):

    GREEN     ^T0 GREEN  and  ^T1-GATE GREEN  and  ^T2-GAMEPLAY GREEN
    RED       any of ^T0 RED/REFUSED, ^T1-GATE RED, ^T2-GAMEPLAY RED/REFUSED
    UNJUDGED  neither -- killed, infra, or refused before a stage ran

**UNJUDGED IS NOT RED, AND KEEPING THEM APART IS THE WHOLE CARE HERE.** 67 of
the 225 gate logs on this box reached no stage verdict at all: they were killed
mid-run, died on ssh, or were refused for local state. Counting those as failures
would drag p from 0.79 to 0.56 and would be measuring how often this project's
agents get interrupted, not how often a branch is good. That is the same
distinction `ci/handoff.sh` grew its `INFRA` verdict for.

Measured 2026-08-17 over 225 logs on sayer-desktop:

    GREEN 125   RED 33   UNJUDGED 67   ->   p = 125/158 = 0.791

which corroborates the briefed 0.81 rather than the 0.60 the easy instrument
gave. Both are reported by `--compare` so the gap stays visible.

USAGE
    ci/gate-rate.py                      # p over every gate log
    ci/gate-rate.py --since 20260814     # only gates stamped on/after that day
    ci/gate-rate.py --dir <path>
    ci/gate-rate.py --arith              # the ci/batch-arith.py line to run
    ci/gate-rate.py --selftest           # red-when-broken evidence

`--selftest` builds logs whose verdicts are known — including a truncated one
that must come out UNJUDGED rather than RED — and asserts the classifier on each.
A classifier nobody can show misreading a log is a classifier that cannot fail.
"""

import argparse
import os
import re
import sys
import tempfile

DEFAULT_DIR = os.path.expanduser("~/rustmc-private/gates")
# ci/batch.sh's enable gate, from docs/GATE-BATCHING.md.
P_FLOOR = 0.70

# A rate over a handful of gates is not a rate. Same discipline as
# ci/corpus-floor.sh refusing on fewer than two references: print the N beside
# the figure, and refuse to call it a rate below this.
MIN_N = 20

GREEN_SENTINELS = (
    re.compile(r"^T0 GREEN", re.M),
    re.compile(r"^T1-GATE GREEN", re.M),
    re.compile(r"^T2-GAMEPLAY GREEN", re.M),
)
# THE STAGE SENTINELS DO NOT SEE A T2 FAILURE — 2026-08-18.
#
# `ci/t2-gameplay.sh` contains no `T2-GAMEPLAY RED` string at all: it prints
# `T2-GAMEPLAY GREEN` on success, `T2-GAMEPLAY REFUSED` when it declines to
# measure, and on a genuine corpus regression it simply EXITS NON-ZERO.
# `ci/handoff.sh` reads that as `T2_RC` (line 773) and reports it in its own
# summary line — `RED <branch> — sync=.. t0=0/1 t1=0/1 t2=1/0 ..` — and never
# prints a `T2-GAMEPLAY RED` line anywhere.
#
# So the three stage sentinels this classifier was built from are blind to a
# failure of the stage that is 66% of the gate. Measured: four logs carry
# handoff's own RED summary and were being classified UNJUDGED, and three of
# them are real T2 failures — `chain-cascade-signs` (twice) and
# `gate-skips-and-scenario-verbs`, all at `t2=1/0`.
#
# That is rule 7 exactly: T2 appeared never to have failed in 143 runs, and the
# reason was the measurement, not the world. Handoff's own summary line is
# added as a sentinel because it is the ONE place a verdict is stated
# regardless of which stage produced it.
RED_SENTINELS = re.compile(
    r"^(?:T0 (?:RED|REFUSED)|T1-GATE RED|T2-GAMEPLAY (?:RED|REFUSED)"
    r"|RED\s+\S+\s+—)", re.M
)
# The stage marker PLUS the reason handoff prints after it. The reason is what
# makes two REDs the same failure rather than merely the same stage, and
# find_incidents needs that distinction to tell a main-side breakage from a
# branch that happened to fail at the same time.
RED_SIGNATURE = re.compile(
    r"^((?:T0 (?:RED|REFUSED)|T1-GATE RED|T2-GAMEPLAY (?:RED|REFUSED))[^\n|]{0,60})", re.M
)
# A gate whose only RED evidence is handoff's summary gets its signature from
# the per-stage rcs in that line, so a T2 failure is distinguishable from a
# stage-3 one in the incident grouping rather than collapsing into "RED".
RED_SUMMARY_SIG = re.compile(r"^RED\s+\S+\s+—\s+(.*?)\s+no-vanilla-skips", re.M)
STAMP = re.compile(r"-(\d{8})-\d{6}\.log$")
# gate-<branch>-<YYYYMMDD>-<HHMMSS>.log. The branch name is everything between,
# and branch names here contain '-' freely, so anchor on the stamp at the end
# rather than splitting on the separator.
NAMED = re.compile(r"^gate-(.+)-(\d{8})-(\d{6})\.log$")


def parse_name(name):
    """-> (branch, 'YYYYMMDDHHMMSS') or (None, None) for an unparseable name."""
    m = NAMED.match(name)
    if not m:
        return None, None
    return m.group(1), m.group(2) + m.group(3)


def _mins(stamp):
    """Minutes since epoch-ish, for windowing. Calendar-correct to the day."""
    import datetime
    return int(datetime.datetime.strptime(stamp, "%Y%m%d%H%M%S").timestamp() // 60)


def classify(text):
    if all(s.search(text) for s in GREEN_SENTINELS):
        return "GREEN"
    if RED_SENTINELS.search(text):
        return "RED"
    return "UNJUDGED"


def scan(directory, since=None):
    counts = {"GREEN": 0, "RED": 0, "UNJUDGED": 0}
    per_file = []
    if not os.path.isdir(directory):
        return counts, per_file, f"no such directory: {directory}"
    for name in sorted(os.listdir(directory)):
        if not (name.startswith("gate-") and name.endswith(".log")):
            continue
        if since:
            m = STAMP.search(name)
            if not m or m.group(1) < since:
                continue
        path = os.path.join(directory, name)
        try:
            with open(path, "r", errors="replace") as fh:
                text = fh.read()
        except OSError:
            continue
        v = classify(text)
        counts[v] += 1
        branch, stamp = parse_name(name)
        sig = ""
        if v == "RED":
            m1 = RED_SIGNATURE.search(text)
            if m1:
                sig = m1.group(1).strip()
            else:
                m2 = RED_SUMMARY_SIG.search(text)
                sig = ("handoff summary — " + m2.group(1).strip()) if m2 else "RED (summary)"
        per_file.append({"name": name, "verdict": v, "branch": branch,
                         "stamp": stamp, "signature": sig})
    return counts, per_file, None


def report(counts, out=sys.stdout, label=""):
    g, r, u = counts["GREEN"], counts["RED"], counts["UNJUDGED"]
    n = g + r
    out.write(f"gate-rate{label}: {g} GREEN, {r} RED, {u} UNJUDGED "
              f"over {g + r + u} logs\n")
    if n == 0:
        out.write("gate-rate: no log reached a stage verdict — no rate\n")
        return None
    p = g / n
    out.write(f"gate-rate: p = {g}/{n} = {p:.4f}   (UNJUDGED excluded: "
              f"they are not verdicts about a branch)\n")
    if n < MIN_N:
        out.write(f"gate-rate: N = {n} is below {MIN_N} — quote it as an estimate, "
                  f"not as the rate\n")
    if u > n:
        out.write(f"gate-rate: NOTE more logs were UNJUDGED ({u}) than reached a "
                  f"verdict ({n}); counting them as RED would give "
                  f"{g / (g + r + u):.4f} and would be measuring interruptions\n")
    return p


def per_branch(records):
    """One verdict per BRANCH — its most recent — instead of one per log.

    WHY THE PER-LOG RATE IS BIASED, AND IN WHICH DIRECTION
    ------------------------------------------------------
    `p` is used by ci/batch-arith.py as the probability that an arbitrary
    QUEUED BRANCH passes its gate. The per-log rate answers a different
    question: the probability that an arbitrary GATE RUN comes out green. Those
    differ whenever a branch is gated more than once, and the difference is not
    noise — it has a sign.

    A branch that goes RED stays in the queue and is gated again, usually with
    the same defect, so it contributes several REDs. A branch that goes GREEN
    merges and is never gated again, so it contributes exactly one GREEN.
    Retries are therefore drawn almost entirely from the failing population,
    and the per-log rate is biased DOWNWARD by construction.

    Measured 2026-08-18 over ~/rustmc-private/gates:
        since 15 Aug   per-log 22/39 = 0.5641   per-branch 22/33 = 0.6667
        all history    per-log 127/160 = 0.7937 per-branch 122/135 = 0.9037
    Five branches account for the whole of the recent gap, each gated twice
    with the same signature both times.

    The most recent attempt is the one kept, not the first and not the best: it
    is the branch's current state, which is what a batch would actually pick up.
    """
    latest = {}
    for r in records:
        if r["verdict"] not in ("GREEN", "RED") or not r["branch"]:
            continue
        prev = latest.get(r["branch"])
        if prev is None or (r["stamp"] or "") >= (prev["stamp"] or ""):
            latest[r["branch"]] = r
    return list(latest.values())


def find_incidents(records, min_branches=5, gap_min=30):
    """Group REDs into main-side incidents.

    WHAT AN INCIDENT IS, AND WHY IT IS NOT A VERDICT ABOUT A BRANCH
    ---------------------------------------------------------------
    A branch defect is a property of one branch. When many DIFFERENT branches
    reach RED within a few minutes of each other, the thing they have in common
    is not a defect each of them independently acquired — it is main, or the
    box, or a check that main broke. `ci/handoff.sh` already refuses to score
    that class when it can see it, and calls it INFRA. It cannot see this one:
    the gate really did run, a stage really did print RED, and the branch is
    charged for its input's defect.

    THE INSTANCE THIS WAS BUILT FROM, measured 2026-08-18 over 48 logs since
    15 Aug. 14 of the 17 REDs fall in one 27-minute window on 16 Aug,
    11:13-11:38, across 12 DISTINCT branches — and they carry FOUR distinct
    signatures, not one:

        7x  T0 RED — a gameplay check carries no evidence it can go red
        4x  T0 RED — a published figure and its artefact disagree
        2x  T0 RED — a doc claim collided, or drifted from its heading
        1x  T1-GATE RED — 2 problem(s)

    That plurality is the point, and it corrects the account this work was
    briefed with ("twelve branches all died on FIGURES RED — 37 failing
    rules"). One branch-side defect cannot produce four different failure
    signatures across twelve branches in 27 minutes; four simultaneous
    main-side breakages can, and did. Excluding that window takes the recent
    rate from 0.5641 (N=39) to 0.8750 (N=24).

    THE THRESHOLD IS ON DISTINCT BRANCHES, NOT ON LOGS, AND THAT IS THE WHOLE
    CARE HERE. One branch retried six times in twenty minutes is an ordinary
    retry storm and must NOT be excused as an incident — that would let any
    persistently broken branch launder its own REDs. `selftest` asserts it.

    Returns a list of {"start","end","branches","logs","signatures"}.
    """
    reds = sorted((r for r in records
                   if r["verdict"] == "RED" and r["stamp"]),
                  key=lambda r: r["stamp"])
    groups, cur = [], []
    for r in reds:
        if cur and _mins(r["stamp"]) - _mins(cur[-1]["stamp"]) > gap_min:
            groups.append(cur)
            cur = []
        cur.append(r)
    if cur:
        groups.append(cur)
    out = []
    for g in groups:
        # A RED belongs to the incident only if ITS OWN signature is carried by
        # two or more DISTINCT branches in the window. Being inside the window
        # is not enough, and this is the guard that stops the rule laundering
        # real failures: on 16 Aug `server-power-through-set-block` failed
        # `T1-GATE RED — 2 problem(s)` at 10:52 and again at 11:15, inside the
        # window and with a signature no other branch carried. That is a branch
        # defect that merely coincided with a main-side outage, and a window-
        # wide exclusion would have credited it as GREEN — turning this from a
        # correction into a way for any branch to hide behind a bad morning.
        by_sig = {}
        for x in g:
            by_sig.setdefault(x["signature"], set()).add(x["branch"])
        shared = {sig for sig, brs in by_sig.items() if len(brs) >= 2}
        members = [x for x in g if x["signature"] in shared]
        brs = {x["branch"] for x in members if x["branch"]}
        if len(brs) >= min_branches:
            out.append({"start": members[0]["stamp"], "end": members[-1]["stamp"],
                        "branches": sorted(brs),
                        "signatures": sorted(shared),
                        "logs": [x["name"] for x in members]})
    return out


def selftest():
    """Known-verdict logs, including the ones that must NOT be read as RED."""
    fails = []
    cases = {
        # name                              content                          want
        "gate-a-20260817-101010.log": ("T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY GREEN\n", "GREEN"),
        "gate-b-20260817-101011.log": ("T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY RED\n", "RED"),
        "gate-c-20260817-101012.log": ("T0 GREEN\nT1-GATE RED\n", "RED"),
        "gate-d-20260817-101013.log": ("T0 REFUSED\n", "RED"),
        # Truncated mid-run: two stages green and then nothing. This is the one
        # that matters -- it is an interruption, not a failing branch, and
        # calling it RED is how p gets dragged down by how often agents get
        # killed.
        "gate-e-20260817-101014.log": ("T0 GREEN\nT1-GATE GREEN\n", "UNJUDGED"),
        # Empty: the gate died before stage 1 printed anything.
        "gate-f-20260817-101015.log": ("", "UNJUDGED"),
        # A stage name inside prose must not be mistaken for a sentinel: the
        # patterns are anchored at line start for exactly this.
        "gate-g-20260817-101016.log": ("see T2-GAMEPLAY RED in the other log\n"
                                       "T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY GREEN\n", "GREEN"),
        # A T2 REFUSED after all three greens would be contradictory; GREEN wins
        # because handoff's own condition is the conjunction of the three.
        "gate-h-20260817-101017.log": ("T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY GREEN\n"
                                       "T2-GAMEPLAY REFUSED\n", "GREEN"),
    }
    with tempfile.TemporaryDirectory() as d:
        for name, (content, want) in cases.items():
            with open(os.path.join(d, name), "w") as fh:
                fh.write(content)
        for name, (content, want) in sorted(cases.items()):
            got = classify(content)
            if got != want:
                fails.append(name)
                print(f"  FAIL  {name}: classified {got}, want {want}")
            else:
                print(f"  PASS  {name}: {want}")

        counts, per_file, err = scan(d)
        if err:
            fails.append("scan")
            print(f"  FAIL  scan: {err}")
        want_counts = {"GREEN": 3, "RED": 3, "UNJUDGED": 2}
        if counts != want_counts:
            fails.append("counts")
            print(f"  FAIL  counts: {counts}, want {want_counts}")
        else:
            print(f"  PASS  counts: {counts}")

        # --since must actually filter, and a file whose name carries no stamp
        # must be excluded rather than silently included.
        counts2, _, _ = scan(d, since="20260818")
        if sum(counts2.values()) != 0:
            fails.append("since")
            print(f"  FAIL  --since did not filter: {counts2}")
        else:
            print("  PASS  --since filters by the stamp in the filename")

        with open(os.path.join(d, "gate-nostamp.log"), "w") as fh:
            fh.write("T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY GREEN\n")
        counts3, _, _ = scan(d, since="20260101")
        if counts3["GREEN"] != 3:
            fails.append("nostamp")
            print(f"  FAIL  a stampless filename was counted under --since: {counts3}")
        else:
            print("  PASS  a stampless filename is excluded under --since")

        # And the rate itself: 3 green of 6 verdicts is 0.5, and the 2 UNJUDGED
        # must not appear in the denominator.
        p = report(counts, out=open(os.devnull, "w"))
        if p is None or abs(p - 0.5) > 1e-9:
            fails.append("rate")
            print(f"  FAIL  rate: {p}, want 0.5 (UNJUDGED must be out of the denominator)")
        else:
            print("  PASS  rate excludes UNJUDGED from the denominator")

    # ---- per-branch and incident detection -------------------------------
    #
    # Every case below writes REAL log files and goes through scan(), so the
    # signature extraction is exercised end to end rather than hand-fed. The
    # two guards (cases I3 and I4) are the ones that matter: without them the
    # incident rule is a way for a persistently failing branch to launder its
    # own REDs behind somebody else's bad morning, which would be a correction
    # that can only ever move p upward.
    def mk(d, entries):
        # scan() silently skips a filename it cannot parse, so a case whose
        # stamp is malformed produces ZERO records and every "is NOT an
        # incident" assertion passes vacuously. That happened while these cases
        # were being written -- I3 and I5 went green against an empty list --
        # and it is this project's central defect class appearing inside the
        # test written to avoid it. So: refuse the fixture rather than the
        # assertion, and refuse it here where the name is built.
        for br, stamp, body in entries:
            with open(os.path.join(d, f"gate-{br}-{stamp}.log"), "w") as fh:
                fh.write(body)
        recs = scan(d)[1]
        assert len(recs) == len(entries), (
            f"fixture built {len(entries)} logs but scan() parsed {len(recs)}")
        for r in recs:
            assert r["branch"] and r["stamp"], f"unparseable fixture name {r['name']}"
        return recs

    GRN = "T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY GREEN\n"
    def red(reason):
        return f"T0 RED — {reason}\n"

    def expect(label, cond):
        if cond:
            print(f"  PASS  {label}")
        else:
            fails.append(label)
            print(f"  FAIL  {label}")

    # B1: a branch gated twice counts once, and it is the LATEST that counts.
    with tempfile.TemporaryDirectory() as d:
        recs = mk(d, [("alpha", "20260816-100000", red("figures")),
                      ("alpha", "20260816-120000", GRN),
                      ("beta",  "20260816-100000", GRN)])
        pb = per_branch(recs)
        expect("B1: a branch gated twice contributes one verdict",
               len(pb) == 2)
        expect("B1: and it is the MOST RECENT one (red then green -> green)",
               tally(pb) == {"GREEN": 2, "RED": 0, "UNJUDGED": 0})
        expect("B1: per-log would have said 2/3 — the bias is real and downward",
               tally(recs)["RED"] == 1)

    # B2: green then red keeps the red — per_branch is 'current state', not
    # 'best ever'. Without this, per_branch would only ever raise p.
    with tempfile.TemporaryDirectory() as d:
        recs = mk(d, [("alpha", "20260816-100000", GRN),
                      ("alpha", "20260816-120000", red("figures"))])
        expect("B2: green then red keeps the RED (not 'best ever')",
               tally(per_branch(recs)) == {"GREEN": 0, "RED": 1, "UNJUDGED": 0})

    # I1: the real shape — many branches, one shared signature, one window.
    with tempfile.TemporaryDirectory() as d:
        recs = mk(d, [(f"br{i}", f"20260816-1113{i:02d}", red("main is broken"))
                      for i in range(6)])
        incs = find_incidents(recs)
        expect("I1: 6 branches, one signature, one window -> incident",
               len(incs) == 1 and len(incs[0]["branches"]) == 6)

    # I2: the same REDs spread over six days are six ordinary failures.
    with tempfile.TemporaryDirectory() as d:
        recs = mk(d, [(f"br{i}", f"2026081{i}-111300", red("main is broken"))
                      for i in range(6)])
        expect("I2: the same REDs spread over days are NOT an incident",
               find_incidents(recs) == [])

    # I3: THE LAUNDERING GUARD. One branch retried eight times inside twenty
    # minutes is a retry storm, not a main-side outage. The threshold counts
    # DISTINCT BRANCHES for exactly this.
    with tempfile.TemporaryDirectory() as d:
        recs = mk(d, [("solo", f"20260816-1113{i:02d}", red("its own defect"))
                      for i in range(8)])
        expect("I3: one branch retried 8x in 20 min is NOT an incident",
               find_incidents(recs) == [])

    # I4: THE SECOND LAUNDERING GUARD, and the one a real log needed. A branch
    # failing on its OWN signature inside a genuine incident window is not
    # excused: on 16 Aug server-power-through-set-block carried
    # `T1-GATE RED — 2 problem(s)`, which no other branch carried, and a
    # window-wide exclusion would have credited it as green.
    with tempfile.TemporaryDirectory() as d:
        recs = mk(d, [(f"br{i}", f"20260816-1113{i:02d}", red("main is broken"))
                      for i in range(6)]
                   + [("solo", "20260816-111400", "T1-GATE RED — 2 problem(s)\n")])
        incs = find_incidents(recs)
        expect("I4: a solo signature inside the window is NOT excused",
               len(incs) == 1 and "solo" not in incs[0]["branches"])
        expect("I4: and its log is not dropped from the denominator",
               all("solo" not in lg for lg in incs[0]["logs"]))

    # I5: the threshold binds. Four branches at a floor of five is not an
    # incident, so the rule cannot be satisfied by any small coincidence.
    with tempfile.TemporaryDirectory() as d:
        recs = mk(d, [(f"br{i}", f"20260816-1113{i:02d}", red("main is broken"))
                      for i in range(4)])
        expect("I5: 4 distinct branches at a floor of 5 is NOT an incident",
               find_incidents(recs) == [])
        expect("I5: and lowering the floor to 4 DOES find it (the knob is live)",
               len(find_incidents(recs, min_branches=4)) == 1)

    # I6: two signatures shared by two branches each still make one incident —
    # the 16 Aug event carried four distinct signatures, so a rule keyed to a
    # single shared signature would have missed most of it.
    with tempfile.TemporaryDirectory() as d:
        recs = mk(d, [("a", "20260816-111300", red("figures")),
                      ("b", "20260816-111310", red("figures")),
                      ("c", "20260816-111320", red("claims")),
                      ("d", "20260816-111330", red("claims")),
                      ("e", "20260816-111340", red("gameplay evidence")),
                      ("f", "20260816-111350", red("gameplay evidence"))])
        incs = find_incidents(recs)
        expect("I6: several distinct shared signatures form ONE incident",
               len(incs) == 1 and len(incs[0]["signatures"]) == 3
               and len(incs[0]["branches"]) == 6)

    # ---- the T2 blind spot ------------------------------------------------
    # ci/t2-gameplay.sh has no `T2-GAMEPLAY RED` string; a corpus regression
    # exits non-zero and only handoff's own summary line records it. These are
    # verbatim in the shape of gate-cascade-signs.log.
    T2FAIL = ("T0 GREEN\nT1-GATE GREEN\n"
              "RED      chain-cascade-signs — t0=0/1 tables=0 t1=0/1 t2=1/0 "
              "no-vanilla-skips=0\n")
    expect("T1: a T2 failure (t2=1/0) is RED, not UNJUDGED",
           classify(T2FAIL) == "RED")
    expect("T1: and the old stage-only sentinels could NOT see it",
           not re.compile(r"^(?:T0 (?:RED|REFUSED)|T1-GATE RED"
                          r"|T2-GAMEPLAY (?:RED|REFUSED))", re.M).search(T2FAIL))
    expect("T1: a MERGED summary is not dragged in by the new sentinel",
           classify("T0 GREEN\nT1-GATE GREEN\nT2-GAMEPLAY GREEN\n"
                    "MERGED   somebranch -> main  +2 commits\n") == "GREEN")
    expect("T1: the word RED in prose does not match (anchored, needs the dash)",
           classify("the RED herring here is not a verdict\n") == "UNJUDGED")
    with tempfile.TemporaryDirectory() as d:
        with open(os.path.join(d, "gate-cascade-20260805-101010.log"), "w") as fh:
            fh.write(T2FAIL)
        r = scan(d)[1][0]
        expect("T1: its signature names the stage rcs, not just 'RED'",
               "t2=1/0" in r["signature"])

    # ---- measured gate cost ----------------------------------------------
    # Durations come from mtime minus the filename stamp, so the fixtures set
    # mtime explicitly and the assertion is on a known number of minutes.
    import datetime as _dt
    with tempfile.TemporaryDirectory() as d:
        def put(br, stamp, body, minutes):
            fp = os.path.join(d, f"gate-{br}-{stamp}.log")
            with open(fp, "w") as fh:
                fh.write(body)
            t0 = _dt.datetime.strptime(stamp.replace("-", ""), "%Y%m%d%H%M%S")
            os.utime(fp, (0, (t0 + _dt.timedelta(minutes=minutes)).timestamp()))
        put("g1", "20260816-100000", GRN, 60)
        put("g2", "20260816-110000", GRN, 70)
        put("r1", "20260816-120000", red("figures"), 1)
        put("r2", "20260816-130000", "T1-GATE RED — 2 problem(s)\n", 21)
        # 9 hours: a file somebody touched, not a gate somebody ran.
        put("x1", "20260816-140000", GRN, 540)
        c = measure_costs(d, scan(d)[1])
        expect("K1: green cost is the mean of the green fixtures (65 min)",
               abs(sum(c["GREEN"]) / len(c["GREEN"]) - 65.0) < 1e-6)
        expect("K1: and the 9-hour outlier is dropped, not averaged in",
               len(c["GREEN"]) == 2)
        expect("K2: red cost is the mean of the red fixtures (11 min)",
               abs(sum(c["RED"]) / len(c["RED"]) - 11.0) < 1e-6)
        expect("K3: T0 and T1 reds are separated, because they differ 100-fold",
               len(c["RED/T0 RED"]) == 1 and len(c["RED/T1-GATE RED"]) == 1)
        expect("K3: and the T0 red is the cheap one",
               c["RED/T0 RED"][0] < c["RED/T1-GATE RED"][0])

    print(f"\ngate-rate selftest: {'GREEN' if not fails else 'RED'} ({len(fails)} failed)")
    return 1 if fails else 0


def measure_costs(directory, records):
    """What a gate actually COSTS, by verdict — in minutes, from the logs.

    WHY THIS EXISTS. ci/batch-arith.py counts GATES, and until 2026-08-18 a
    gate was one unit whatever it did. It is not: measured over 229 logs here,

        GREEN        n=127   mean 65.2 min
        RED          n= 33   mean  3.4 min
          T0 RED       n=28  mean  0.2 min   (twelve seconds)
          T1-GATE RED  n= 5  mean 21.3 min
          T2 RED       n= 0  -- there has never been one

    A failure costs 0.052 of a success, because ci/t0.sh's tree-only checks run
    first and preflight.sh now catches most of that class before a slot is even
    taken. Feeding these two numbers to `ci/batch-arith.py --cost-green
    --cost-red` moves the optimum from width 3 / batch_singles / 1.175x to
    width 8 / bisect / ~2.04x — the STRATEGY changes, not just the ratio,
    because bisect's extra gates are precisely the ones that come out red.

    THE INSTRUMENT, AND ITS ONE ASSUMPTION. Duration is the log's mtime minus
    the stamp in its own filename. ci/handoff.sh writes continuously and stops
    when the gate stops, so mtime is when it last had anything to say. A log
    still being written reads as "so far", which is why a currently-running
    gate should be excluded rather than trusted; anything over 6 h is dropped
    as a file somebody touched rather than a gate somebody ran.

    Note what is NOT in this population: there is no T2 RED anywhere in it. So
    the measured red cost is extrapolating over a failure mode batching is more
    exposed to than serial is, and that is the honest limit of the figure —
    see docs/GATE-BATCHING.md §2.3.
    """
    import datetime
    out = {}
    for r in records:
        if not r["stamp"]:
            continue
        try:
            mt = os.path.getmtime(os.path.join(directory, r["name"]))
        except OSError:
            continue
        start = datetime.datetime.strptime(r["stamp"], "%Y%m%d%H%M%S")
        dur = (datetime.datetime.fromtimestamp(mt) - start).total_seconds() / 60.0
        if dur < 0 or dur > 360:
            continue
        key = r["verdict"]
        out.setdefault(key, []).append(dur)
        if r["verdict"] == "RED" and r["signature"]:
            stage = r["signature"].split("—")[0].strip()
            out.setdefault(f"RED/{stage}", []).append(dur)
    return out


def report_costs(costs):
    print(f"  {'verdict':<28}{'n':>5}{'mean min':>10}{'median':>9}{'max':>8}")
    for k in sorted(costs, key=lambda k: (k.startswith("RED/"), k)):
        v = sorted(costs[k])
        med = v[len(v) // 2]
        print(f"  {k:<28}{len(v):>5}{sum(v)/len(v):>10.1f}{med:>9.1f}{max(v):>8.1f}")
    g = costs.get("GREEN"); r = costs.get("RED")
    if g and r:
        cg, cr = sum(g)/len(g), sum(r)/len(r)
        print(f"\n  a RED costs {cr/cg:.3f} of a GREEN — ci/batch-arith.py prices "
              f"both at 1.000 unless told otherwise\n")
        print(f"  ./ci/batch-arith.py --p <p> --queue <n> "
              f"--cost-green {cg:.1f} --cost-red {cr:.1f}")


def tally(records):
    c = {"GREEN": 0, "RED": 0, "UNJUDGED": 0}
    for r in records:
        c[r["verdict"]] += 1
    return c


def describe_incidents(incs):
    if not incs:
        print("gate-rate: no main-side RED incident found in this window")
        return
    for i in incs:
        print(f"gate-rate: INCIDENT {i['start']}..{i['end']} — "
              f"{len(i['logs'])} REDs across {len(i['branches'])} distinct branches")
        print(f"           {' '.join(i['branches'])}")
        for sig in i["signatures"]:
            n = sum(1 for lg in i["logs"] if lg)
            print(f"           sig: {sig}")


def decide(records, a):
    """Answer ci/batch.sh's enable gate: is p >= P_FLOOR at N >= MIN_N?

    Printed under every definition rather than one, because the definitions
    disagree and the disagreement is the finding. A single number here would be
    a choice presented as a measurement.
    """
    print(f"ci/batch.sh enable gate: p >= {P_FLOOR:.2f} at N >= {MIN_N}\n")
    rows = []

    def line(label, recs):
        c = tally(recs)
        n = c["GREEN"] + c["RED"]
        if n == 0:
            print(f"  {label:<44} no verdicts")
            return
        p = c["GREEN"] / n
        met = p >= P_FLOOR and n >= MIN_N
        why = "" if met else ("  <- N too small" if n < MIN_N else "  <- p below floor")
        print(f"  {label:<44} p = {c['GREEN']:3d}/{n:<3d} = {p:.4f}  "
              f"{'MET' if met else 'not met'}{why}")
        rows.append(met)

    incs = find_incidents(records, a.min_branches, a.gap_min)
    drop = {n for i in incs for n in i["logs"]}
    no_inc = [r for r in records if r["name"] not in drop]

    line("all history, per-log", records)
    line("all history, per-branch", per_branch(records))
    recent = [r for r in records if (r["stamp"] or "") >= "20260815000000"]
    line("since 15 Aug, per-log", recent)
    line("since 15 Aug, per-branch", per_branch(recent))
    recent_ni = [r for r in no_inc if (r["stamp"] or "") >= "20260815000000"]
    line("since 15 Aug, per-log, incidents excluded", recent_ni)
    line("since 15 Aug, per-branch, incidents excluded", per_branch(recent_ni))

    print()
    describe_incidents(incs)
    print()
    if rows and all(rows):
        print("gate-rate: the enable gate's p criterion is MET under every "
              "definition that clears N >= 20.")
    elif any(rows):
        print("gate-rate: the criterion is met under some definitions and not "
              "others — read the incident list above before choosing one.")
    else:
        print("gate-rate: the criterion is NOT met under any definition.")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dir", default=DEFAULT_DIR)
    ap.add_argument("--since", default=None, help="YYYYMMDD; uses the stamp in the filename")
    ap.add_argument("--arith", action="store_true",
                    help="print the ci/batch-arith.py invocation for the measured p")
    ap.add_argument("--list", action="store_true", help="one line per log")
    ap.add_argument("--per-branch", action="store_true",
                    help="one verdict per branch (its most recent) instead of one per log")
    ap.add_argument("--incidents", action="store_true",
                    help="report main-side RED incidents and the rate with them excluded")
    ap.add_argument("--decide", action="store_true",
                    help="answer ci/batch.sh's enable gate under every definition of p")
    ap.add_argument("--min-branches", type=int, default=5,
                    help="distinct branches in one window to call it an incident (default 5)")
    ap.add_argument("--gap-min", type=int, default=30,
                    help="minutes between REDs that keeps them in one incident (default 30)")
    ap.add_argument("--cost", action="store_true",
                    help="measure what a gate costs in minutes, by verdict")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    counts, per_file, err = scan(a.dir, a.since)
    if err:
        print(f"gate-rate: {err}", file=sys.stderr)
        return 2
    if a.list:
        for r in per_file:
            print(f"  {r['verdict']:<9s} {r['name']}")
    label = f" (since {a.since})" if a.since else ""

    if a.cost:
        report_costs(measure_costs(a.dir, per_file))
        return 0

    if a.decide:
        return decide(per_file, a)

    records = per_file
    if a.per_branch:
        records = per_branch(per_file)
        counts = tally(records)
        label += " [per-branch]"
    if a.incidents:
        incs = find_incidents(per_file, a.min_branches, a.gap_min)
        describe_incidents(incs)
        drop = {n for i in incs for n in i["logs"]}
        records = [r for r in records if r["name"] not in drop]
        counts = tally(records)
        label += " [incidents excluded]"

    p = report(counts, label=label)
    if p is None:
        return 1
    if a.arith:
        print(f"\n  ./ci/batch-arith.py --p {p:.4f} --queue "
              f"{counts['GREEN'] + counts['RED']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
