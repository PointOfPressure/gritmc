#!/usr/bin/env python3
"""What a batch of width w is worth, exactly, at a measured green-at-verdict rate.

WHY THIS FILE EXISTS, AND WHY IT IS NOT A SPREADSHEET
=====================================================
`ci/lanes.sh` is a merge train: it runs N gates concurrently and bases each
lane on its predecessor's *gated* tip, so every queued branch still gets its
own full gate.  Its ceiling is the pipelining ceiling (~1.40x measured,
`docs/CORPUS-LOCK-HOLD.md`) and no arrangement of it lands two branches for one
gate.

BATCHING is the other operation: several branches onto one integration tip, ONE
gate, one fast-forward that lands them all.  `ci/batch.sh` does that.  Whether
it is worth doing, and at what width, is decided entirely by the number below,
and it is not the number intuition offers:

    The tempting reading is "13 branches for 1 gate, so 13x".
    The true reading is that a batch lands NOTHING unless EVERY member is
    green, and at the MEASURED per-branch green-at-verdict rate of 0.7911
    (N=158, ci/gate-rate.py) a 13-wide batch passes 5.4% of the time.  It is
    WORSE than serial.

AND THE RATE IS NOT A CONSTANT, WHICH IS THE MOST IMPORTANT LINE HERE.
`ci/gate-rate.py --since` on this box, 2026-08-17:

    all history  p = 0.7911  (N=158)   ->  batch, width 3
    since 08-10  p = 0.7000  (N=100)   ->  break-even
    since 08-12  p = 0.5890  (N= 73)   ->  DO NOT BATCH
    since 08-14  p = 0.5000  (N= 56)   ->  DO NOT BATCH
    since 08-17  p = 0.9167  (N= 12)   ->  batch, but N is below 20

`--break-even` computes where the line is: **below p = 0.6934 no width beats
serial at all**, and width 3 is the first to pay.  So "should we batch" is a
comparison against a measured number, not a judgement, and the answer on this
box today depends entirely on which window is the right population.  See §2 of
`docs/GATE-BATCHING.md`.

So the failure path is the design, and this script scores it exactly -- by
enumerating all 2**w good/bad patterns and weighting them, not by simulation
and not by algebra that can be wrong quietly.

THE MODEL, STATED SO IT CAN BE DISAGREED WITH
=============================================
  * Each member is independently GOOD with probability p.  "Good" means the
    gate would return GREEN for that branch alone.
  * A gate over a group returns GREEN iff every member is good.
  * A gate over w members costs `1 + width_cost*(w-1)` units.  With
    `--width-cost 0` (the default) a batch gate is exactly as expensive as a
    single-branch gate: one rsync, one cargo build, one corpus run over a
    slightly bigger tip.  That is the assumption batching's whole win rests
    on, so it is the first thing to attack -- `--width-cost` exists to price
    the attack rather than argue about it.
  * Every good member eventually lands, under every strategy here, so expected
    landings are w*p throughout and the only figure that separates strategies
    is GATES PER LANDED BRANCH.  Serial is 1/p by construction.

THE STRATEGIES, AND WHY ONE OF THEM IS REFUSED
==============================================
  `serial`         one gate per branch.  The baseline: 1/p per landed branch.
  `batch_singles`  gate the batch; on RED, re-gate every member singly.
                   Equivalently: on RED the batch dissolves back into the
                   serial queue.  Every member ends with its own verdict.
  `bisect`         gate the batch; on RED, split into halves and recurse.
                   A singleton RED is that member's own verdict.

  `batch_singles_infer` -- REFUSED, AND THE REASON IS THE POINT.  After a RED
  batch, if the first w-1 members each gate GREEN, it is tempting to conclude
  the last member is bad without gating it, saving a gate.  That inference is
  false in exactly the case batching creates: an INTERACTION RED, where two
  individually-green branches are red only in combination.  Condemning a
  branch on it would be a verdict that no measurement of that branch produced
  -- this project's central defect class (AGENT-BRIEF, HARD RULE).  It is
  computed below and printed as an UNSOUND lower bound only, so that the price
  of refusing it is visible instead of assumed.  `ci/batch.sh` does not
  implement it: it reports interaction reds and lands neither member.

WHAT THIS DOES NOT MODEL, SAID PLAINLY
  * Interaction reds themselves.  They make every batching strategy worse than
    stated, because splitting finds no culprit: both halves gate green and the
    members are only landable apart.  This is a second reason the honest
    headline is a NARROW batch.
  * Collision-infeasible groups.  A member whose commits will not cherry-pick
    onto the integration tip never joins the batch at all; `ci/batch.sh`
    applies that before any gate runs, and it caps width from the other side.
  * Correlated failure (a broken main breaks every member at once), which
    would make batching better than stated.

USAGE
    ci/batch-arith.py                       # table at the measured p=0.7911
    ci/batch-arith.py --break-even          # the p below which batching loses
    ci/batch-arith.py --p 0.95 --queue 15
    ci/batch-arith.py --width-cost 0.10     # price a 10%-per-member premium
    ci/batch-arith.py --crossover           # where bisect starts to pay
    ci/batch-arith.py --selftest            # closed-form checks; red-able

`--selftest` is this script's red-when-broken evidence.  Its identities are
closed forms no arithmetic slip survives: E(1)=1 at every p; E(w)=1 at p=1;
bisect costs 2w-1 and batch_singles costs w+1 at p=0; serial costs exactly
1/p per landed branch; and the two batching strategies coincide at w<=2, where
halving a pair IS gating both singly.
"""

import argparse
import sys
from itertools import product

# The MEASURED per-branch green-at-verdict rate, with its N. Not the briefed
# 0.81 and not invented here: ci/gate-rate.py classifies every gate log
# ci/handoff.sh has ever written by handoff's own stage sentinels, and gets
# 125 GREEN / 33 RED / 67 UNJUDGED over 225 logs. A rate without an N is not a
# rate, and this one comes with a WINDOW too -- see ci/gate-rate.py's header,
# because the trailing 3-day figure is 0.50 and that is on the other side of
# every break-even below.
P_RECORDED = 0.7911
P_RECORDED_N = 158


# MEASURED GATE COSTS, and why they default to 1.0 -- 2026-08-18.
#
# Everything below counts GATES, and until today a gate was one unit whatever
# it did. Measured over 229 logs in ~/rustmc-private/gates:
#
#     GREEN        n=127   mean 65.2 min
#     RED          n= 33   mean  3.4 min      <- not 65.2
#       T0 RED       n=28  mean  0.2 min      (12 seconds)
#       T1-GATE RED  n= 5  mean 21.3 min
#       T2 RED       n= 0  -- there has never been one
#
# So a failure costs 0.052 of a success, not 1.000. Twenty-eight of the
# thirty-three REDs in this project's whole history cost twelve seconds,
# because ci/t0.sh's tree-only checks run first and preflight.sh now catches
# most of that class before a slot is even taken.
#
# THIS MATTERS TO THE ANSWER, NOT ONLY TO THE PRESENTATION. Serial's headline
# weakness is that it pays a gate per branch INCLUDING the ones that fail;
# batching's is that a failed batch pays for follow-ups. Pricing every gate the
# same overstates serial's waste and overstates batching's downside, and the
# two do not cancel.
#
# DEFAULTS ARE 1.0 SO NOTHING ALREADY PUBLISHED MOVES. docs/GATE-BATCHING.md's
# 1.0596 / 1.2641 / 1.175x are all gate-unit figures and remain exactly
# reproducible; --cost-green/--cost-red opt into the wall-clock reading.
COST_GREEN = 1.0
COST_RED = 1.0


def gate_cost(w, width_cost, green=True):
    """Cost of ONE gate over w members, given whether that gate comes out green.

    `green` is never guessed: a gate over a group is green iff every member of
    that group is good, which is `all(group)` at every call site below. It is
    passed explicitly rather than inferred here so that a strategy cannot
    accidentally price a gate by a verdict it does not actually observe -- the
    same discipline the strategies already keep about `pat`.
    """
    return (COST_GREEN if green else COST_RED) * (1.0 + width_cost * (w - 1))


# ---------------------------------------------------------------------------
# THE STRATEGIES, EACH SCORED ON ONE KNOWN PATTERN
# ---------------------------------------------------------------------------
# `pat` is a tuple of booleans, True = that member is good.  Each function
# returns (gates, landed) for that exact pattern.  Nothing here sees `pat` as a
# whole the way a strategy could not: every branch of every function keys only
# off gate VERDICTS, which are `all(group)`.  That is the discipline that keeps
# these from being oracles rather than strategies.

def s_serial(pat, wc):
    # One gate per member, each green iff that member is good. This used to be
    # `len(pat) * gate_cost(1, wc)`, which is identical while COST_RED == 1.0
    # and is what the gate-unit figures were computed from.
    return sum(gate_cost(1, wc, g) for g in pat), sum(pat)


def s_batch_singles(pat, wc):
    w = len(pat)
    gates = gate_cost(w, wc, all(pat))
    if all(pat):
        return gates, w
    if w == 1:
        return gates, 0              # a singleton RED is its own verdict --
        #                              re-gating it would be measuring twice,
        #                              which is what the first version did, and
        #                              selftest identity 5 caught it.
    gates += sum(gate_cost(1, wc, g) for g in pat)
    return gates, sum(pat)


def s_batch_singles_infer(pat, wc):
    """UNSOUND -- see the header.  Priced, not implemented."""
    w = len(pat)
    gates = gate_cost(w, wc, all(pat))
    if all(pat):
        return gates, w
    gates += sum(gate_cost(1, wc, g) for g in pat[:w - 1])
    if all(pat[:w - 1]):
        return gates, w - 1          # last one condemned WITHOUT a measurement
    gates += gate_cost(1, wc, pat[w - 1])
    return gates, sum(pat)


def s_bisect(pat, wc):
    w = len(pat)
    gates = gate_cost(w, wc, all(pat))
    if all(pat):
        return gates, w
    if w == 1:
        return gates, 0              # a singleton RED is its own verdict
    a = (w + 1) // 2
    gl, ll = s_bisect(pat[:a], wc)
    gr, lr = s_bisect(pat[a:], wc)
    return gates + gl + gr, ll + lr


STRATEGIES = [
    ("serial", s_serial, True),
    ("batch_singles", s_batch_singles, True),
    ("bisect", s_bisect, True),
    ("batch_singles_infer", s_batch_singles_infer, False),
]


def expected(strategy, w, p, wc):
    """Exact E[gates], E[landed] over all 2**w patterns."""
    eg = el = 0.0
    for pat in product((True, False), repeat=w):
        pr = 1.0
        for good in pat:
            pr *= p if good else (1.0 - p)
        if pr == 0.0:
            continue
        g, l = strategy(pat, wc)
        eg += pr * g
        el += pr * l
    return eg, el


def per_landed(strategy, w, p, wc):
    eg, el = expected(strategy, w, p, wc)
    return (eg / el) if el else float("inf")


def report(p, queue, max_width, wc, out=sys.stdout):
    w_ = out.write
    w_(f"green-at-verdict rate p = {p:.4f}   (N = {P_RECORDED_N} branches, preflight.tsv)\n")
    w_(f"queue = {queue} branches;   a w-wide gate costs 1 + {wc}*(w-1) units\n\n")
    w_("           GATES PER LANDED BRANCH (lower is better)          P(batch\n")
    w_("  width   serial  batch_singles     bisect   [unsound]        passes)   best sound\n")
    w_("  " + "-" * 76 + "\n")
    serial_pl = per_landed(s_serial, 1, p, wc)
    best = None
    for w in range(1, max_width + 1):
        bs = per_landed(s_batch_singles, w, p, wc)
        bi = per_landed(s_bisect, w, p, wc)
        un = per_landed(s_batch_singles_infer, w, p, wc)
        cands = (("batch_singles", bs), ("bisect", bi))
        name, val = min(cands, key=lambda t: t[1])
        if best is None or val < best[2]:
            best = (w, name, val)
        w_(f"  {w:5d}   {serial_pl:6.4f}   {bs:12.4f}   {bi:8.4f}   {un:8.4f}   "
           f"{p ** w:12.4f}   {name} {serial_pl / val:.3f}x\n")
    w_("\n")
    w_(f"  serial baseline: {serial_pl:.4f} gates per landed branch\n")
    w_(f"  BEST SOUND: width {best[0]} under {best[1]} -- {best[2]:.4f} per landed, "
       f"{serial_pl / best[2]:.3f}x serial\n")
    # The whole-queue figure at the chosen width, which is what the owner reads.
    strat = dict((n, f) for n, f, _ in STRATEGIES)[best[1]]
    nfull, rem = divmod(queue, best[0])
    tot = nfull * expected(strat, best[0], p, wc)[0]
    landed = nfull * expected(strat, best[0], p, wc)[1]
    if rem:
        g, l = expected(strat, rem, p, wc)
        tot += g
        landed += l
    ser = expected(s_serial, 1, p, wc)[0] * queue
    w_(f"  whole queue of {queue}: {tot:.2f} gates landing {landed:.2f} branches, "
       f"against {ser:.2f} serial gates landing {queue * p:.2f}\n")
    w_(f"  at 80 min a gate: {tot * 80 / 60:.1f} h against {ser * 80 / 60:.1f} h "
       f"-- saves {(ser - tot) * 80 / 60:.1f} h\n")
    return best


def advise(p, width, wc=0.0, out=sys.stdout):
    """One machine-readable line: at this p and this width, batch or go serial?

    Exists so `ci/batch.sh` can consult the arithmetic instead of carrying a
    copy of its threshold. A hardcoded 0.6934 in a shell script is a figure
    that would go stale the first time this model changed, which is the failure
    mode `docs/AGENT-BRIEF.md` calls a caveat list that lags the code.
    """
    # `ser` WAS `1/p`, WHICH IS THE SERIAL COST IN GATES — 2026-08-19. Once
    # COST_GREEN/COST_RED carry minutes, `per_landed` is in minutes and `1/p`
    # is still in gates, so the comparison that produces the verdict compared
    # two different quantities: at p=0.8217 priced 65.2/3.4 this printed
    # `per_landed=51.9181 serial=1.2170 verdict=SERIAL`, recommending serial
    # over a configuration worth 2.221x. Built from the same unit now.
    ser = per_landed(s_serial, 1, p, wc)
    # AND IT ONLY EVER CONSIDERED `batch_singles`, so `best_width` could not
    # see bisect — which is what wins at every width above 2 once a RED is
    # priced at a nineteenth of a GREEN. The verdict is taken over both.
    best_w, best_v, best_s = 1, ser, "serial"
    for w in range(1, max(2, width + 1)):
        for nm, f in (("batch_singles", s_batch_singles), ("bisect", s_bisect)):
            v = per_landed(f, w, p, wc)
            if v < best_v:
                best_w, best_v, best_s = w, v, nm
    at_width = min(per_landed(s_batch_singles, width, p, wc),
                   per_landed(s_bisect, width, p, wc))
    verdict = "BATCH" if at_width < ser else "SERIAL"
    unit = "min" if (COST_GREEN, COST_RED) != (1.0, 1.0) else "gates"
    out.write(f"ADVISE p={p:.4f} width={width} per_landed={at_width:.4f} "
              f"serial={ser:.4f} unit={unit} best_width={best_w} "
              f"best_strategy={best_s} verdict={verdict}\n")
    return verdict


def break_even(wc=0.0, out=sys.stdout, lo=0.30, hi=0.999, tol=1e-4):
    """The p at which each width first beats serial, by bisection.

    This is the number that turns "should we batch" from a judgement into a
    comparison.  Serial costs 1/p per landed branch at every p, so a width is
    worth using exactly when its own per-landed figure drops below that, and
    the crossing is monotone in p because a batch's pass probability p**w is.

    It exists because the measured rate MOVES.  `ci/gate-rate.py` puts the
    trailing rate on this box between 0.50 and 0.92 depending on the window, and
    those two answers are on opposite sides of every threshold below.
    """
    out.write("\n  BREAK-EVEN: the p at which each width first beats serial\n")
    out.write("  width   p*      at p* it is worth\n")
    out.write("  " + "-" * 44 + "\n")
    best_overall = None
    for w in range(2, 9):
        a, b = lo, hi
        if per_landed(s_batch_singles, w, hi, wc) >= (1.0 / hi):
            out.write(f"  {w:5d}   never   never beats serial at any p <= {hi}\n")
            continue
        for _ in range(60):
            m = (a + b) / 2
            if per_landed(s_batch_singles, w, m, wc) < (1.0 / m):
                b = m
            else:
                a = m
            if b - a < tol:
                break
        sp = (1.0 / b) / per_landed(s_batch_singles, w, b, wc)
        out.write(f"  {w:5d}   {b:.4f}  {sp:.3f}x\n")
        if best_overall is None:
            best_overall = (w, b)
    if best_overall:
        out.write(f"\n  SO: below p = {best_overall[1]:.4f} NO width beats serial and "
                  f"batching costs more than it saves.\n")
        out.write(f"      The first width to pay is {best_overall[0]}.\n")
    return best_overall


def _parse_groups(spec):
    """`--groups` -> a list of sizes, or None having said why on stderr."""
    try:
        gs = [int(x) for x in spec.split(",") if x.strip()]
    except ValueError:
        print(f"batch-arith: --groups '{spec}' is not a comma-separated list of sizes",
              file=sys.stderr)
        return None
    if not gs or any(g < 1 or g > 18 for g in gs):
        print("batch-arith: --groups sizes must be between 1 and 18", file=sys.stderr)
        return None
    return gs


def observed(groups, collisions, p, wc, out=sys.stdout):
    """Score an ACTUAL group structure, as `ci/batch.sh --plan` reported it.

    The table above answers "what is width w worth"; this answers "what is the
    queue in front of me worth", which is a different question because the real
    groups are not all width w.  A member that will not cherry-pick onto its
    group's tip is excluded before any gate runs, so the observed structure is
    narrower than the requested width and the saving is correspondingly
    smaller.  Feeding the measurement back in is the only way that shows up.

    `collisions` still cost one serial gate each; they are counted so the
    denominator is the whole queue rather than the part that batched.
    """
    w_ = out.write
    members = sum(groups)
    ser_members = members + collisions

    # A gate that comes out RED costs 3.4 minutes and one that comes out GREEN
    # costs 65.2, so `gate_cost` is already verdict-weighted through the
    # COST_GREEN/COST_RED globals and every total below is in whatever unit
    # those carry. The line under this used to read `at 80 min a gate`, flat,
    # for both verdicts and for a figure that is not even the measured green
    # cost. That is the "a gate is not one unit" error this option exists to
    # correct, committed by the function that reports the correction.
    priced = (COST_GREEN, COST_RED) != (1.0, 1.0)
    unit = "min" if priced else "gates"

    # AND IT SCORED ONLY `batch_singles` — 2026-08-19. `strat` was hardcoded,
    # so the observed structure could not be scored under `bisect` at all, and
    # bisect is what the cost-weighted optimum selects at every width above 2.
    # An instrument that can only report the strategy you are not going to use
    # is answering a question nobody asked.
    rows = []
    for name, strategy in (("batch_singles", s_batch_singles), ("bisect", s_bisect)):
        tot = landed = 0.0
        for g in groups:
            eg, el = expected(strategy, g, p, wc)
            tot += eg
            landed += el
        # A collision is excluded before any gate runs and is then gated alone,
        # so it costs one serial gate — which is `gate_cost` at width 1 over
        # its own verdict, not a flat unit.
        coll_cost = collisions * (COST_GREEN + ((1.0 - p) / p) * COST_RED) if p > 0 \
            else float("inf")
        rows.append((name, tot, landed, tot + coll_cost))

    ser_one = (COST_GREEN + ((1.0 - p) / p) * COST_RED) if p > 0 else float("inf")
    ser = ser_members * ser_one

    w_(f"\nOBSERVED group structure: {len(groups)} groups over {members} members "
       f"({','.join(str(g) for g in groups)})\n")
    w_(f"  mean group {members / len(groups):.2f} against the requested width — "
       f"the gap is collision structure, not the arithmetic\n")
    w_(f"  plus {collisions} member(s) excluded as COLLISION, one serial gate each\n")
    best = min(rows, key=lambda r: r[3])
    for name, tot, landed, tot_all in rows:
        mark = "  <-- best" if name == best[0] else ""
        w_(f"  {name:>14}: batched {tot:8.2f} {unit} landing {landed:.2f} of {members}; "
           f"whole queue {tot_all:8.2f} against {ser:8.2f} serial -- "
           f"{ser / tot_all:.3f}x{mark}\n")
    if priced:
        name, _, _, tot_all = best
        w_(f"  in hours, {name} over the whole queue of {ser_members}: "
           f"{tot_all / 60:.1f} h against {ser / 60:.1f} h -- "
           f"saves {(ser - tot_all) / 60:.1f} h\n")
    else:
        w_("  (unpriced: pass --cost-green/--cost-red to price this in minutes; "
           "in gates a RED and a GREEN count the same and they do not cost the same)\n")
    # This line said `gates per landed branch` while `tot_all` was in whatever
    # unit COST_GREEN carries, so under `--cost-green 65.2` it printed 46.52
    # "gates" against serial's 1.22 — two different quantities compared as one,
    # which is the same units defect the block above corrects. It names its
    # unit now, and the serial comparator is built from the same one.
    name, _, landed_b, tot_all_b = best
    w_(f"  {unit} per landed branch, {name}: "
       f"{tot_all_b / (landed_b + collisions * p):.4f} "
       f"against serial's {ser_one:.4f}\n")


def crossover(p_lo=0.5, p_hi=0.999, steps=100, wc=0.0, out=sys.stdout):
    """The rate at which each width's best strategy flips, and where bisect pays.

    Printed because the recorded 0.81 is not a constant of nature: it is this
    queue's rate today.  If the queue's green rate rises -- and every merge
    driver that lands raises it -- the optimum widens, and the owner should be
    able to see how far without re-deriving anything.
    """
    out.write("\n  p      best sound width / strategy      per landed   vs serial   "
              "bisect first pays at width\n")
    out.write("  " + "-" * 88 + "\n")
    for i in range(steps + 1):
        p = p_lo + (p_hi - p_lo) * i / steps
        if i % max(1, steps // 12):
            continue
        best = None
        first_bisect = "-"
        for w in range(1, 17):
            bs = per_landed(s_batch_singles, w, p, wc)
            bi = per_landed(s_bisect, w, p, wc)
            if bi < bs and first_bisect == "-":
                first_bisect = str(w)
            name, val = min((("batch_singles", bs), ("bisect", bi)), key=lambda t: t[1])
            if best is None or val < best[2]:
                best = (w, name, val)
        sp = (1.0 / p) / best[2]
        out.write(f"  {p:.3f}  width {best[0]:2d} / {best[1]:<14s}   "
                  f"{best[2]:8.4f}    {sp:6.3f}x     {first_bisect}\n")


def selftest():
    fails = []

    def check(name, got, want, tol=1e-9):
        if abs(got - want) > tol:
            fails.append(name)
            print(f"  FAIL  {name}: got {got}, want {want}")
        else:
            print(f"  PASS  {name}")

    # 1. A singleton costs exactly one gate at any rate, under every strategy.
    for p in (0.0, 0.5, 0.81, 1.0):
        for n, f, _ in STRATEGIES:
            check(f"{n}: E(1) gates == 1 at p={p}", expected(f, 1, p, 0.0)[0], 1.0)

    # 2. At p=1 nothing ever splits, so any width costs one gate and lands all.
    for w in (1, 2, 3, 13):
        for n, f, _ in STRATEGIES:
            g, l = expected(f, w, 1.0, 0.0)
            if n == "serial":
                check(f"serial: E({w}) gates == {w} at p=1", g, float(w))
            else:
                check(f"{n}: E({w}) gates == 1 at p=1", g, 1.0)
            check(f"{n}: E({w}) landed == {w} at p=1", l, float(w))

    # 3. At p=0: bisect splits to singletons, so the full binary tree over w
    #    leaves -- 2w-1 nodes, one gate each.  batch_singles pays 1 + w.
    for w in (1, 2, 3, 4, 5, 13):
        check(f"bisect: E({w}) == 2w-1 at p=0", expected(s_bisect, w, 0.0, 0.0)[0],
              float(2 * w - 1))
        check(f"batch_singles: E({w}) == w+1 at p=0",
              expected(s_batch_singles, w, 0.0, 0.0)[0], float(w + 1) if w > 1 else 1.0)

    # 4. Serial costs exactly 1/p per landed branch, at every rate.
    for p in (0.5, 0.81, 0.9):
        check(f"serial per-landed == 1/p at p={p}", per_landed(s_serial, 1, p, 0.0), 1.0 / p)

    # 5. The two batching strategies COINCIDE at w<=2: halving a pair is
    #    gating both singly, so any divergence there is an implementation slip
    #    in one of them rather than a fact about batching.
    for w in (1, 2):
        for p in (0.3, 0.81, 0.97):
            check(f"bisect == batch_singles at w={w}, p={p}",
                  per_landed(s_bisect, w, p, 0.0), per_landed(s_batch_singles, w, p, 0.0))

    # 6. THE FINDING, held as a test so a later edit cannot quietly undo it:
    #    at the recorded rate, halving on RED is WORSE than dissolving back to
    #    serial, for every width above 2.  This is the assertion the first
    #    version of this file got backwards.
    for w in (3, 4, 5, 8, 13):
        bi = per_landed(s_bisect, w, P_RECORDED, 0.0)
        bs = per_landed(s_batch_singles, w, P_RECORDED, 0.0)
        if not bi > bs:
            fails.append(f"bisect should be worse at w={w}")
            print(f"  FAIL  bisect worse than batch_singles at w={w}: {bi:.4f} !> {bs:.4f}")
        else:
            print(f"  PASS  bisect worse than batch_singles at w={w}: {bi:.4f} > {bs:.4f}")

    # 7. And bisect DOES pay at a high enough rate, so #6 is a statement about
    #    p=0.81 and not about the split being pointless code.
    hp = 0.98
    bi = per_landed(s_bisect, 13, hp, 0.0)
    bs = per_landed(s_batch_singles, 13, hp, 0.0)
    if not bi < bs:
        fails.append("bisect should pay at p=0.98, w=13")
        print(f"  FAIL  bisect pays at p={hp}, w=13: {bi:.4f} !< {bs:.4f}")
    else:
        print(f"  PASS  bisect pays at p={hp}, w=13: {bi:.4f} < {bs:.4f}")

    # 8. The unsound strategy is genuinely cheaper -- which is WHY it is
    #    tempting and why refusing it has to be a decision rather than an
    #    oversight.  If it ever stops being cheaper, the refusal costs nothing
    #    and this line should say so.
    un = per_landed(s_batch_singles_infer, 3, P_RECORDED, 0.0)
    bs = per_landed(s_batch_singles, 3, P_RECORDED, 0.0)
    if not un < bs:
        fails.append("unsound infer should be cheaper at w=3")
        print(f"  FAIL  unsound infer cheaper at w=3: {un:.4f} !< {bs:.4f}")
    else:
        print(f"  PASS  unsound infer cheaper at w=3 ({un:.4f} < {bs:.4f}) "
              f"-- refusing it costs {bs - un:.4f} gates per landed branch")

    # ---- measured gate costs -------------------------------------------
    #
    # These guard a change that CAN silently move every published figure in
    # docs/GATE-BATCHING.md, so the first thing asserted is that it does not.
    def with_costs(cg, cr, fn_, w, pp, wc=0.0):
        g0, r0 = COST_GREEN, COST_RED
        globals()["COST_GREEN"], globals()["COST_RED"] = cg, cr
        try:
            return per_landed(fn_, w, pp, wc)
        finally:
            globals()["COST_GREEN"], globals()["COST_RED"] = g0, r0

    def truth(name, cond):
        if cond:
            print(f"  PASS  {name}")
        else:
            fails.append(name)
            print(f"  FAIL  {name}")

    # C1: THE IDENTITY. Defaults are 1.0/1.0 and every gate-unit figure this
    # project has published must be reproduced to the digit. Without this, the
    # cost change is free to move docs/GATE-BATCHING.md's numbers unnoticed.
    # P_RECORDED (0.7911, N=158) is deliberately NOT updated to the 0.7937 /
    # N=160 that ci/gate-rate.py measures today: it is the snapshot every
    # published figure in docs/GATE-BATCHING.md was computed from, and moving
    # it would silently restate them for a change of 0.0026. gate-rate.py is
    # the live instrument; this constant is the provenance of the doc.
    check("C1: defaults reproduce the published batch_singles w=3 = 1.0596",
          per_landed(s_batch_singles, 3, P_RECORDED, 0.0), 1.0596, tol=5e-5)
    check("C1: defaults reproduce the published serial = 1.2641",
          per_landed(s_serial, 1, P_RECORDED, 0.0), 1.2641, tol=5e-5)
    truth("C1: explicit 1.0/1.0 is identical to the default",
          abs(with_costs(1.0, 1.0, s_bisect, 8, P_RECORDED)
              - per_landed(s_bisect, 8, P_RECORDED, 0.0)) < 1e-12)

    # C2: THE KNOB IS LIVE, and in the direction the mechanism predicts. A
    # cheap RED helps bisect more than batch_singles, because bisect's extra
    # gates are exactly the ones that come out red.
    unit_bs = per_landed(s_batch_singles, 8, 0.7937, 0.0)
    unit_bi = per_landed(s_bisect, 8, 0.7937, 0.0)
    meas_bs = with_costs(65.2, 3.4, s_batch_singles, 8, 0.7937)
    meas_bi = with_costs(65.2, 3.4, s_bisect, 8, 0.7937)
    truth("C2: in GATE UNITS batch_singles beats bisect at w=8",
          unit_bs < unit_bi)
    truth("C2: in MEASURED MINUTES bisect beats batch_singles at w=8",
          meas_bi < meas_bs)
    truth("C2: and that reversal is the whole finding — it is not a rescaling",
          (unit_bs < unit_bi) and (meas_bi < meas_bs))

    # C3: THE CONTROL. Make a RED cost MORE than a GREEN and bisect must become
    # the worse strategy again. A knob that only ever moves one way is not
    # measuring anything.
    truth("C3: with an EXPENSIVE red, bisect is worse than batch_singles again",
          with_costs(65.2, 200.0, s_bisect, 8, 0.7937)
          > with_costs(65.2, 200.0, s_batch_singles, 8, 0.7937))

    # C4: the unit model is the CONSERVATIVE end, not a wrong answer. Setting
    # red = green must reproduce the unit ratio exactly, so every published
    # figure is the case "a failure costs as much as a success".
    truth("C4: red == green reproduces the unit ratio exactly",
          abs(with_costs(65.2, 65.2, s_batch_singles, 3, 0.7925) / 65.2
              - per_landed(s_batch_singles, 3, 0.7925, 0.0)) < 1e-12)

    # C5: serial is unchanged by width and must still price correctly:
    # one green gate plus (1-p)/p cheap reds per landed branch.
    pp = 0.7937
    truth("C5: serial in minutes == cost_green + ((1-p)/p)*cost_red",
          abs(with_costs(65.2, 3.4, s_serial, 1, pp)
              - (65.2 + (1 - pp) / pp * 3.4)) < 1e-9)

    # 9. THE UNIT DECIDES THE STRATEGY, AND `--groups` MUST SEE THE PRICE.
    #
    # Both halves of this were broken on 2026-08-19 and each hid the other.
    # `observed()` scored only `batch_singles`, and `--groups` was dropped
    # without a word when `--cost-green` was supplied, so the one invocation
    # that answers "what is the queue in front of me actually worth" answered
    # nothing at all.
    #
    # What it costs is not a rounding error: on the real 60-branch queue of
    # 2026-08-19, planned at width 8 into groups 5,6,5,5,8,4,4,4 with 19
    # collisions, the WINNER INVERTS with the unit. Counting gates, singles
    # wins and bisect is worse than it. Pricing a RED at 3.4 minutes against a
    # GREEN's 65.2, bisect wins by a fifth of the whole queue. A batch that
    # fails is nearly free and that is the entire argument for a wide one.
    import io
    groups, coll, pp = [5, 6, 5, 5, 8, 4, 4, 4], 19, 0.8217

    g0, r0 = COST_GREEN, COST_RED
    try:
        globals()["COST_GREEN"], globals()["COST_RED"] = 1.0, 1.0
        buf = io.StringIO(); observed(groups, coll, pp, 0.0, out=buf)
        unpriced = buf.getvalue()
        globals()["COST_GREEN"], globals()["COST_RED"] = 65.2, 3.4
        buf = io.StringIO(); observed(groups, coll, pp, 0.0, out=buf)
        priced = buf.getvalue()
    finally:
        globals()["COST_GREEN"], globals()["COST_RED"] = g0, r0

    def _best(text):
        for line in text.splitlines():
            if "<-- best" in line:
                return line.strip().split(":")[0].strip()
        return "<none>"

    truth("C6: counting gates, batch_singles wins the observed structure",
          _best(unpriced) == "batch_singles")
    truth("C7: pricing red at 3.4 against green at 65.2, bisect wins instead",
          _best(priced) == "bisect")
    truth("C8: both strategies are scored, not just the hardcoded one",
          "bisect" in unpriced and "batch_singles" in unpriced)
    truth("C9: the priced report states its unit as minutes, not gates",
          "min landing" in priced and "gates landing" in unpriced)

    # 10. `--advise` IS THE LINE A SHELL SCRIPT ACTS ON, so its units must
    # agree with themselves. It compared `per_landed` — in minutes once priced
    # — against `1/p`, which is in gates, and so returned SERIAL for a
    # configuration worth 2.221x. It also scored only `batch_singles`, so
    # `best_width` could not see the strategy that wins once a RED is priced
    # at a nineteenth of a GREEN. `ci/batch.sh` reads the verdict as
    # `${adv##*verdict=}`, so `verdict=` must stay the last field.
    g0, r0 = COST_GREEN, COST_RED
    try:
        buf = io.StringIO()
        globals()["COST_GREEN"], globals()["COST_RED"] = 65.2, 3.4
        advise(0.8217, 8, 0.0, out=buf)
        priced_line = buf.getvalue().strip()
        buf = io.StringIO()
        globals()["COST_GREEN"], globals()["COST_RED"] = 1.0, 1.0
        advise(0.8217, 8, 0.0, out=buf)
        unpriced_line = buf.getvalue().strip()
    finally:
        globals()["COST_GREEN"], globals()["COST_RED"] = g0, r0

    truth("C10: priced, --advise says BATCH at the width the table calls best",
          "verdict=BATCH" in priced_line and "best_width=8" in priced_line
          and "best_strategy=bisect" in priced_line)
    truth("C11: and it states its unit, so the two numbers cannot be read as one",
          "unit=min" in priced_line and "unit=gates" in unpriced_line)
    truth("C12: unpriced, the advice ci/batch.sh already acts on is unchanged",
          "best_width=3" in unpriced_line and "verdict=BATCH" in unpriced_line)
    truth("C13: verdict= stays LAST — ci/batch.sh reads ${adv##*verdict=}",
          priced_line.split("verdict=")[-1] == "BATCH"
          and unpriced_line.split("verdict=")[-1] == "BATCH")

    print(f"\nbatch-arith selftest: {'GREEN' if not fails else 'RED'} ({len(fails)} failed)")
    return 1 if fails else 0


def wallclock(p, queue, max_width, wc, cg, cr):
    """The same comparison, priced in measured minutes instead of gate-units.

    A gate-unit answer assumes every gate costs the same. It does not: a GREEN
    gate is 65.2 min and a RED is 3.4, because 28 of 33 REDs in this project's
    history died at T0 in twelve seconds. Both strategies' costs move when that
    is priced, and they do not move by the same factor -- which is the reason
    this exists rather than being a presentational rescaling of the unit table.
    """
    w_ = sys.stdout.write
    w_(f"priced in MEASURED MINUTES: green gate {cg:.1f} min, red gate {cr:.1f} min "
       f"(ratio {cr / cg:.3f})\n")
    w_(f"p = {p:.4f}   queue = {queue} branches\n\n")
    w_(f"  {'width':>5}  {'strategy':<16}{'min/landed':>12}{'vs serial':>11}\n")
    ser = None
    rows = []
    for width in range(1, max_width + 1):
        for name, fn_, sound in STRATEGIES:
            if not sound:
                continue
            if width == 1 and name != "serial":
                continue
            if width > 1 and name == "serial":
                continue
            eg, el = expected(fn_, width, p, wc)
            if el <= 0:
                continue
            v = eg / el
            if name == "serial":
                ser = v
            rows.append((width, name, v))
    for width, name, v in rows:
        ratio = (ser / v) if (ser and v) else float("nan")
        star = "  <-- best" if abs(v - min(r[2] for r in rows)) < 1e-12 else ""
        w_(f"  {width:>5}  {name:<16}{v:>12.2f}{ratio:>10.3f}x{star}\n")
    best = min(rows, key=lambda r: r[2])
    w_(f"\n  best: {best[1]} at width {best[0]} -- {best[2]:.2f} min per landed "
       f"branch against serial's {ser:.2f} ({ser / best[2]:.3f}x)\n")
    w_(f"  over a {queue}-branch queue: {queue * best[2] / 60:.1f} h "
       f"against serial's {queue * ser / 60:.1f} h "
       f"(saves {queue * (ser - best[2]) / 60:.1f} h)\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--p", type=float, default=P_RECORDED)
    ap.add_argument("--queue", type=int, default=15)
    ap.add_argument("--max-width", type=int, default=13)
    ap.add_argument("--width-cost", type=float, default=0.0,
                    help="extra gate cost per member beyond the first (default 0)")
    ap.add_argument("--crossover", action="store_true")
    ap.add_argument("--advise", action="store_true",
                    help="one machine-readable line: BATCH or SERIAL at --p and --width")
    ap.add_argument("--width", type=int, default=3,
                    help="the width --advise judges (default 3)")
    ap.add_argument("--break-even", action="store_true",
                    help="the p at which each width first beats serial")
    ap.add_argument("--groups", default="",
                    help="observed group sizes, e.g. 3,3,1,2 -- from ci/batch.sh --plan")
    ap.add_argument("--collisions", type=int, default=0,
                    help="members --plan excluded as COLLISION; one serial gate each")
    ap.add_argument("--cost-green", type=float, default=None,
                    help="measured minutes for a gate that comes out GREEN "
                         "(ci/gate-rate.py --cost measures it; 65.2 as of 2026-08-18)")
    ap.add_argument("--cost-red", type=float, default=None,
                    help="measured minutes for a gate that comes out RED "
                         "(3.4 as of 2026-08-18 -- NOT the same as --cost-green)")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return selftest()
    if (a.cost_green is None) != (a.cost_red is None):
        print("batch-arith: --cost-green and --cost-red must be given together "
              "-- pricing one and not the other is the error this whole option "
              "exists to fix", file=sys.stderr)
        return 2
    if a.cost_green is not None:
        if a.cost_green <= 0 or a.cost_red < 0:
            print("batch-arith: costs must be positive (red may be 0)", file=sys.stderr)
            return 2
        globals()["COST_GREEN"] = a.cost_green
        globals()["COST_RED"] = a.cost_red
        # `--advise`, `--groups`, `--break-even` and `--crossover` USED TO BE
        # SILENTLY DROPPED HERE — 2026-08-19. This branch returned immediately
        # after `wallclock`, so every one of them was accepted, ignored, and
        # not mentioned. `--groups ... --cost-green 65.2 --cost-red 3.4`
        # printed the generic width table and nothing about the groups;
        # `--advise --cost-green ...` printed a TABLE where the caller is
        # parsing ONE machine-readable line, which `ci/batch.sh` does.
        #
        # The observed structure is the only input carrying the real collision
        # rate and the pricing is the only thing that makes a RED cheap, so the
        # invocations that needed both were the ones that answered neither.
        if a.advise:
            if not 0.0 < a.p <= 1.0:
                print(f"batch-arith: --advise needs 0 < p <= 1, got {a.p}", file=sys.stderr)
                return 2
            if a.width < 1 or a.width > 18:
                print(f"batch-arith: --advise width {a.width} out of range", file=sys.stderr)
                return 2
            advise(a.p, a.width, a.width_cost)
        if a.groups:
            gs = _parse_groups(a.groups)
            if gs is None:
                return 2
            observed(gs, a.collisions, a.p, a.width_cost)
        if a.break_even:
            break_even(wc=a.width_cost)
        if a.crossover:
            crossover(wc=a.width_cost)
        return 0
    if a.advise:
        if not 0.0 < a.p <= 1.0:
            print(f"batch-arith: --advise needs 0 < p <= 1, got {a.p}", file=sys.stderr)
            return 2
        if a.width < 1 or a.width > 18:
            print(f"batch-arith: --advise width {a.width} out of range", file=sys.stderr)
            return 2
        advise(a.p, a.width, a.width_cost)
        return 0
    if not 0.0 <= a.p <= 1.0:
        print(f"batch-arith: --p {a.p} is not a probability", file=sys.stderr)
        return 2
    if a.max_width > 18:
        print("batch-arith: --max-width above 18 enumerates 2**19 patterns; refusing",
              file=sys.stderr)
        return 2
    report(a.p, a.queue, a.max_width, a.width_cost)
    if a.groups:
        gs = _parse_groups(a.groups)
        if gs is None:
            return 2
        observed(gs, a.collisions, a.p, a.width_cost)
    if a.break_even:
        break_even(wc=a.width_cost)
    if a.crossover:
        crossover(wc=a.width_cost)
    return 0


if __name__ == "__main__":
    sys.exit(main())
