# GATE BATCHING — several green branches, one gate

Built 2026-08-17. **Not enabled.** `--land` is off by default and switching it
on is the owner's decision; the checklist for that is §6.

Everything here is reproducible from the tree. No figure in this document was
carried from a note.

---

## 1. What this is, and what `ci/lanes.sh` is

`ci/handoff.sh` is the only route to main and it costs ~80 minutes. With 37
branches queued and all 37 GREEN in `preflight.tsv`, one gate per branch is
~45 hours of gate.

**`ci/lanes.sh` is a merge train and it does not batch.** It runs `LANES` gates
concurrently and bases each train position on its predecessor's *gated* tip:

    position 1: base = origin/main    position 2: base = position 1's gated tip

so every queued branch still gets its own full gate. That is pipelining. Its
ceiling is the pipelining ceiling — ~1.40x, 1.46x at N=2 and 1.72x at N=3 with
the reference cache on, from the 21.3/21.6 min in-lock split in
`docs/CORPUS-LOCK-HOLD.md` — and it is already counted.

**Batching is a different operation:** N branches cherry-picked onto ONE
throwaway integration tip, ONE gate over that tip, one fast-forward that lands
all N. `ci/batch.sh` does that and nothing else in this repo did.

The two compose rather than compete: a lane could drain batches instead of
branches, and the gains multiply. Nothing here assumes or requires that.

## 2. The width is 3, and it is computed

> **Read §2.3 before quoting a width.** Everything in this section counts
> GATES, and a gate is not one unit: measured 2026-08-18, a GREEN gate is
> 65.2 min and a RED is 3.4. Priced in minutes the optimum moves to **width 8
> with `bisect`**. The width-3 answer below is the reading under "a failure
> costs as much as a success", which is the conservative end and not the
> measured one.

A batch lands **nothing** unless **every** member is green. At the **measured**
green-at-verdict rate p = 0.7911 (N = 158 — see §2.1, it is not the briefed 0.81
and it is not a constant):

| width | gates per landed branch | P(batch passes) |
|------:|------------------------:|----------------:|
| 1 | 1.2641 — the serial baseline, 1/p | 0.7911 |
| 2 | 1.1050 | 0.6258 |
| **3** | **1.0596 — best, 1.193x serial** | 0.4951 |
| 4 | 1.0850 | 0.3917 |
| 8 | 1.2282 | 0.1534 |
| 13 | 1.3012 — **worse than serial** | 0.0475 |

`ci/batch-arith.py` computes this exactly by enumerating all 2^w good/bad
patterns — not simulated, not algebra that can be wrong quietly.
`--selftest` is its red-when-broken evidence: eight identity groups in closed
form, and it caught two real slips while being written.

**The collision-feasible width and the throughput-optimal width are different
numbers, and only the second one is a plan.** A 13-wide batch is feasible in the
sense that its members do not textually conflict; it also fails 94% of the time,
pays for the batch gate and then pays for all 13 anyway.

**The lever is p, not width.** At p = 0.90 the best width is 4 (1.68x); at
p = 0.94 it is 8 (2.21x). Every merge driver that lands and every check that
stops being flaky raises p, and raising p is worth more than widening the batch.

## 2.1 p is measured, it is not a constant, and it decides everything

`ci/gate-rate.py` measures p by classifying **every** log `ci/handoff.sh` writes
— one per gate, unconditionally, at `~/rustmc-private/gates/` — using handoff's
own three stage sentinels. `--selftest` is its evidence, including a control that
a log truncated mid-run comes out **UNJUDGED, not RED**: 67 of the 225 logs on
this box reached no stage verdict at all, and counting those as failures would
drag p from 0.79 to 0.56 and be measuring how often agents get interrupted.

`ci/batch-arith.py --break-even` computes the threshold by bisection:

> **Below p = 0.6934, no width beats serial and batching costs more than it
> saves.** Width 3 is the first to pay.

And the measured rate straddles it, depending on the window:

| window | p | N | verdict |
|---|---:|---:|---|
| all history | **0.7911** | 158 | batch, width 3 |
| since 08-10 | 0.7000 | 100 | break-even |
| since 08-12 | 0.5890 | 73 | **do not batch** |
| since 08-14 | 0.5000 | 56 | **do not batch** |
| since 08-17 | 0.9167 | 12 | batch — but N < 20, so not a rate |

**This is the most important thing in this document.** The all-history figure
corroborates the briefed 0.81 and says batch at width 3. The trailing three-day
figure says do not batch at all. They cannot both be the right population, and
which one is depends on something that changed today: the six tree-only preflight
checks and the merge drivers now filter out, in seconds, a class of failure that
used to be discovered 16 seconds into an 80-minute gate slot. Today's twelve
gates went 11 green, which is what that would look like — and twelve gates is not
a rate.

**So the decision rule, and it is a comparison rather than a judgement:**

```bash
./ci/gate-rate.py --since <first day after preflight landed>
```

* **N < 20** — no rate yet. Do not enable. This is where the box stands today.
* **p < 0.70** — do not batch. It costs more than it saves, at every width.
* **p ≥ 0.70** — batch at width 3.
* **p ≥ 0.85** — re-run `./ci/batch-arith.py --p <p>` and widen; the optimum
  moves to 4 and then to 8.

Re-measuring costs one command and no gate. A wrong p does not make the plan
slightly wrong, it makes it the wrong plan.

## 2.2 The re-measurement — 2026-08-18, 229 logs

> **Corrected the same day, and the correction is worth reading before the
> table.** The all-history per-log figure was first published as 0.7937 at
> N=160. It is **0.7744 at N=164**: four gate logs carrying handoff's own
> `RED <branch> — …` summary were being classified UNJUDGED, because the three
> stage sentinels this classifier was built from are blind to a T2 failure —
> `ci/t2-gameplay.sh` has no `T2-GAMEPLAY RED` string and a corpus regression
> simply exits non-zero. Three of the four are real T2 failures. Every verdict
> below survives the correction. See §2.3's closing note.

Everything in §2.1 above stands as written, and the table in it is still the
honest per-log reading of its windows. What follows does not replace it; it
says what those windows were measuring.

Run `./ci/gate-rate.py --decide`:

| definition | p | N | gate |
|---|---:|---:|---|
| all history, per-log | 0.7744 | 164 | **MET** |
| all history, per-branch | 0.9037 | 135 | **MET** |
| since 15 Aug, per-log | 0.5641 | 39 | not met |
| since 15 Aug, per-branch | 0.6667 | 33 | not met |
| since 15 Aug, per-log, incidents excluded | 0.7857 | 28 | **MET** |
| since 15 Aug, per-branch, incidents excluded | 0.8800 | 25 | **MET** |

**The criterion is met at N ≥ 20 under four of six definitions, and the two it
fails are the two that count a log rather than a branch and charge a branch for
main being broken.** Both corrections are argued in `ci/gate-rate.py`'s
docstrings and both are held down by selftest cases that make them refuse to
move p the easy way.

**p is a per-BRANCH rate.** `ci/batch-arith.py` uses p as the chance an
arbitrary queued branch passes. A red branch stays in the queue and is gated
again with the same defect; a green branch merges and is never gated again. So
retries come almost entirely from the failing population and the per-log rate
is biased *downward* by construction. Five branches account for the whole of
the recent gap, each gated twice with the same signature both times.

**The 15-17 Aug dip is one incident.** 14 of the 17 REDs since 15 Aug fall in a
27-minute window on 16 Aug, 11:13-11:38, across 12 distinct branches. They
carry **four** distinct signatures, not one — which corrects the account this
work was briefed with:

| count | signature |
|---:|---|
| 7 | `T0 RED — a gameplay check carries no evidence it can go red` |
| 4 | `T0 RED — a published figure and its artefact disagree` |
| 2 | `T0 RED — a doc claim collided, or drifted from its heading` |
| 1 | `T1-GATE RED — 2 problem(s)` |

One branch-side defect cannot produce four different failure signatures across
twelve branches in 27 minutes. Four simultaneous main-side breakages can, and
the plurality is stronger evidence of a main-side cause than a single shared
signature would have been.

**The exclusion cannot launder a real failure, and that is the only reason it
is worth anything.** A rule that moves p upward has to be harder to satisfy
than the thing it corrects:

* the threshold counts **distinct branches**, so one branch retried eight times
  inside twenty minutes is an ordinary retry storm and is not excused
  (`--selftest` case I3);
* a RED joins the incident only if **its own signature is carried by two or
  more distinct branches** in that window. Being inside the window is not
  enough. `server-power-through-set-block` failed `T1-GATE RED — 2 problem(s)`
  at 10:52 and again at 11:15 — inside the window, with a signature no other
  branch carried. The first version of this rule excused it and reported
  0.9167; the shipped one does not, and reports 0.7857 (`--selftest` case I4).

**What this does and does not authorise.** It closes item 5 of §6 and nothing
else. Items 1-4 are unaffected, `--land` stays off, and enabling batching
remains the owner's switch. The one-line form of the decision is:

> p clears 0.70 at N ≥ 20 on every definition that survives scrutiny; width 3
> stands at 1.151x / ~6.5 h in gate units, and §2.3 prices the same queue at
> width 8 with `bisect` for ~24.6 h; items 1-4 of §6 are still open.

Sensitivity to the assumption that a wide gate costs the same as a narrow one:
width 3 stays optimal at a per-member premium of 0%, 5%, 10% and 20%
(`--width-cost`), so the width recommendation does not rest on that assumption
even though the size of the saving does.

## 2.3 A gate is not one unit — the wall-clock reading

Everything in §2.1 and §2.2 counts **gates**. `ci/batch-arith.py`'s
`gate_cost()` priced every gate at 1.0 whatever it did, and every ratio this
document publishes is in those units. Measured 2026-08-18 over the same 229
logs, with `./ci/gate-rate.py --cost`:

| verdict | n | mean | median |
|---|---:|---:|---:|
| GREEN | 127 | **65.2 min** | 54.3 |
| RED | 33 | **3.4 min** | 0.2 |
| — of which `T0 RED` | 28 | **0.2 min** | 0.2 |
| — of which `T1-GATE RED` | 5 | 21.3 min | 21.0 |
| — of which `T2 RED` | **3 known, unstamped** | not timed | — |
| UNJUDGED | 48 | 22.6 min | 23.9 |

**A failure costs 0.053 of a success.** Twenty-eight of the thirty-three REDs
in this project's whole history cost twelve seconds, because `ci/t0.sh`'s
tree-only checks run first and `preflight.sh` now catches most of that class
before a gate slot is taken at all.

That is not a presentational detail, because serial and batching are exposed to
it differently. Serial's weakness is that it pays a gate per branch including
the failures; batching's is that a failed batch pays for follow-ups. Pricing
both at one unit overstates serial's waste **and** overstates batching's
downside, and the two do not cancel:

```bash
./ci/batch-arith.py --p 0.7744 --queue 46 --cost-green 65.2 --cost-red 3.4
```

| units | best width | best strategy | vs serial | over the 46-branch queue |
|---|---:|---|---:|---|
| gates | 3 | `batch_singles` | 1.151x | ~6.5 h saved |
| **minutes** | **8** | **`bisect`** | **1.943x** | 26.1 h vs 50.7 h — **24.6 h saved** |

**The strategy changes, not only the ratio**, and the mechanism is legible:
`bisect`'s extra gates are exactly the ones that come out red, so `bisect` is
the strategy that benefits when a red is nearly free. Held across sensitivities:

| scenario | best | vs serial | saved |
|---|---|---:|---:|
| measured, `--width-cost 0` | bisect w=8 | 1.943x | 24.6 h |
| a 10%-per-member batch premium | bisect w=8 | 1.736x | 21.5 h |
| pessimistic p = 0.6667 (§2.2's worst definition) | bisect w=4 | 1.564x | 18.5 h |

Every scenario says `bisect` and a width well above 3.

**THIS DOES NOT MAKE §2.1 WRONG — IT MAKES IT THE CONSERVATIVE END.** Setting
`--cost-red` equal to `--cost-green` reproduces the unit answer exactly
(`--selftest` case C4), so 1.151x is the reading under "a failure costs as much
as a success" and 1.943x is the reading under measurement. The defaults are
still 1.0/1.0 and every figure in §2.1 remains reproducible to the digit
(case C1).

It does correct one thing stated earlier: **a 13-wide batch is worse than
serial in gate units and 1.890x better than serial in minutes.** The
"13-wide is worse than serial" line should be read as a statement about gate
units only.

**THE LIMIT OF THIS FIGURE, AND IT IS THE IMPORTANT PARAGRAPH.** The measured
red cost is drawn from a population containing **almost no T2 REDs**. Three are
known — `chain-cascade-signs` twice and `gate-skips-and-scenario-verbs`, all at
`t2=1/0` — and they were invisible until today because
`ci/t2-gameplay.sh` prints no `T2-GAMEPLAY RED` line and only handoff's own
summary records the failure. Their logs carry no filename stamp, so they cannot
be timed and contribute nothing to the 3.4 min mean. T2 is precisely the stage where batching is more exposed than
serial: a batch that fails at T0 costs twelve seconds to bisect, and a batch
that fails after a 43-minute corpus run costs a full gate at every bisect step.
So the 1.943x is conditional on the failure mix staying close to what has been
measured, and the honest reading of the pair is:

> between 1.151x (a failure as expensive as a success) and 1.943x (failures as
> cheap as they have measurably been), with the true value depending on how
> often T2 fails — which is the one stage this data barely constrains.

Both ends beat serial, which is the part that does not depend on the
assumption. **A NOTE ON HOW THIS WAS FOUND, because it is the more useful half.** T2
appeared to have gone green 143 times and red never, in the stage that is 66%
of the gate. That is rule 7 — when a measurement clears every candidate,
suspect the measurement — and it was the measurement: the classifier keyed on
three per-stage strings, and the stage that matters most does not emit one of
them on failure. `ci/gate-rate.py` now also reads handoff's own
`RED <branch> — …` summary, which is the one line that states a verdict
regardless of which stage produced it, and `--selftest` case T1 asserts a
`t2=1/0` log classifies RED **and** that the old sentinels could not see it.

**None of this changes the enable checklist in §6** — items 1-4 are
untouched and `--land` stays off.

## 3. The failure path is the design

At the measured p = 0.7911 a 3-wide batch fails **50%** of the time.

| `--on-red` | what it does | when it wins |
|---|---|---|
| `singles` (default) | the batch dissolves back into the serial queue; every member is re-gated alone | at p = 0.7911, at every width |
| `halves` | split in two and recurse | only above p ≈ 0.94, where it is worth 2.2x |
| `none` | report and stop; members are UNJUDGED, not red | measuring a batch rather than draining one |

Halving is **worse** than dissolving to serial at every width above 2 at the
current rate (1.1621 vs 1.0596 at w=3): a half is itself only all-green 25-50%
of the time, so the interior gate is rarely repaid. At w ≤ 2 the two coincide,
because halving a pair *is* gating both singly. This was asserted backwards in
the first version of `ci/batch-arith.py` and is now held as selftest identity 6.

Three outcomes are kept distinct because they have different fixes:

* **COLLISION** — will not cherry-pick onto the tip. Wants a merge driver. **Not
  a verdict about the branch.**
* **RED** — gated alone and failed. That branch's own verdict.
* **GREEN-INTERACTION** — the batch was RED and every member gates GREEN alone.
  Red only in combination; none is at fault, none is condemned, all are landable
  apart.

**The shortcut this refuses:** after a RED batch, if the first w-1 members gate
GREEN, the last one *must* be bad — so condemn it without gating it. That saves
0.0551 gates per landed branch and is **false** for an interaction red. It would
be a verdict no measurement of that branch produced, which is this project's
central defect class (`docs/AGENT-BRIEF.md`, HARD RULE). `ci/batch-arith.py`
prices it as `[unsound]` so the cost of refusing it is visible rather than
assumed, and selftest identity 8 asserts it really is cheaper — a refusal whose
price nobody can see is indistinguishable from an oversight.

## 4. Grouping: `--pack`, and what it is worth

Slicing the queue every WIDTH branches in arrival order is the obvious grouping
and it is measurably not the best one, because collisions **cluster**: branches
that touch one area arrive together and are therefore sliced into one group.

Measured 2026-08-17 over the 36 live GREEN branches, width 3, against
`origin/main` 8ce434d8, on sayer-desktop:

| grouping | groups | admitted | collisions | gates | vs serial | saved |
|---|---:|---:|---:|---:|---:|---:|
| queue order | 12 | 28 | **8** (22%) | 32.08 | 1.122x | 5.2 h |
| `--pack`, raw disjointness | 27 | 35 | 0 | — | — | — |
| `--pack`, driver-aware | 13 | 35 | **1** | **30.64** | **1.175x** | **7.1 h** |

Gates and savings at the measured p = 0.7911. All three rows are conditional on
p being above the break-even in §2.1, which the trailing window is not.

The middle row is why the third exists. Packing on raw file disjointness removed
every collision and produced **four groups of 3, one of 2 and twenty-two
singletons** — no collisions and almost no batching, a worse outcome than the
collisions were. Nearly every branch touches `docs/WORKLIST.md`'s cold-start
index, a `docs/PARITY-ROADMAP.md` section or `ci/figures.toml`, so once the first
groups fill, every remaining branch shares a path with every existing group.

Those are exactly the files that carry merge drivers, and the drivers exist so
that two branches appending to them do not conflict. Subtracting driver-managed
paths from the collision test takes the queue from 12 groups / 8 collisions to
13 groups / 1 collision, and the saving from 5.2 h to 7.1 h. The theoretical
ideal — all width 3, no collisions — is 30.18 gates and 7.8 h, so driver-aware
packing lands within 0.5 gates of it and there is very little left to win from
better packing.

"Has a driver" is tested as `git config --get merge.<name>.driver`, **not** as
"the attribute is set": an attribute naming a driver git does not have falls back
to the ordinary 3-way merge with no warning, which is the whole reason
`ci/setup-merge-drivers.sh` exists. Treating such a path as non-colliding would
be trusting a promise the checkout has not kept.

The one remaining collision is `worldgen-block-entities`. `ci/t0.sh` carries no
merge driver yet; when it gets `merge=docregions` the feasible width rises again
and this table should be re-taken rather than extrapolated.

Reproduce both rows:

```bash
./ci/batch.sh --plan --width 3 --from-preflight            # queue order
./ci/batch.sh --plan --pack --width 3 --from-preflight     # packed
./ci/batch-arith.py --queue 36 --groups <sizes> --collisions <n>
```

`--plan` runs **no gate**: it builds each group's tip, reports feasibility, and
asks `ci/handoff.sh`'s own state checks whether the tip would be accepted. It
costs about 40 seconds for the whole queue instead of 80 minutes a group.

## 4.1 The bigger saving: one recording, not one gate

This was not in the original brief and it may be worth more than the gate
arithmetic.

`ci/corpus-falsifiable.py`'s own header states the split:

```
--sweep    needs recordings; produces the ledger; seconds; no lock
--check    needs nothing; refuses on the ledger; this is what T0 runs
```

`--check` refuses `UNREGISTERED` — a gameplay scenario with no ledger entry — and
it runs in T0. So **a new scenario cannot pass the tree checks until it appears in
some recording.** The sweep is free. The recording is not: it comes from
`ci/t2-gameplay.sh`, which takes the box-wide corpus lock for ~40 minutes, and
the box runs one at a time.

So the corpus lock is on the critical path for **adding** a scenario, not only for
gating one. That is by design and correct, and `MAX_UNSWEPT` is the deliberately
bounded escape hatch — **do not raise it.** That file says outright that growing
the allowance is a reviewed code change and not something an agent does while
adding the check the allowance would hide.

**The lever is fewer, bigger recordings, and the integration tip already is one.**
A recording covers the scenarios present in the tree it ran against. Today four
agents with new scenarios pay four ~40-minute lock acquisitions; on one tip
carrying all of their scenarios, one recording registers all of them.

| | one gate for N branches | one recording for N branches' scenarios |
|---|---|---|
| saves | 7.1 h on the current queue (§4) | ~40 min per additional branch with new scenarios |
| conditional on | p ≥ 0.70 (§2.1) | nothing — a recording is not a verdict |
| blocks today | nothing | agents that cannot even push |

Note the second column is **not** conditional on p. A batch that gates RED still
had its recording, so this half of the win survives a failure that kills the
other half. That is why the sweep runs *before* the gate.

`ci/sweep-tip.sh` does it for one tree and is useful on its own — an agent with a
new scenario that will not pass `--check` can run it directly. It carries two
refusals that are the point:

1. **It will not take the lock if there is nothing to sweep.** It asks
   `--check` first, which needs no lock and costs a second. Spending 40 minutes to
   discover a tree was already registered is 40 minutes out of the queue of
   somebody who did need it.
2. **It will not queue against a production merge gate.** `ci/handoff.sh:772`
   runs the recorder as `RUSTMC_CORPUS_LOCK_ROLE=gate`, which jumps the whole
   queue because it is the only thing on the box that can move `main`. A
   measurement that declares `gate` competes with production merges as an equal,
   and one did exactly that today: a production gate sat behind an agent's
   measurement for 25 minutes.

   **So this declares `role=run`** — what `docs/AGENT-BRIEF.md` prescribes, and
   what makes a real merge gate jump ahead of it — **and additionally refuses to
   start while a merge gate holds or waits the lock in another tree.** Role `run`
   alone would yield the lock; refusing outright also keeps it off the box while
   the gate is recording, where it would compete for CPU and the reference cache.
   `--allow-gate-contention` overrides and says so.

   Both refusals were verified against live lock state on 2026-08-17 — the guard
   correctly named the real merge gate then holding it (pid 3455242 in
   `/home/sayer/rustmc-refcache-bw-D`) — and both are falsifier cases with
   injections, so neither is a green that cannot go red.

**`ci/batch.sh --sweep`** runs it on the integration tip before the gate and
commits the resulting ledger onto the batch branch. **`--sweep-back`** additionally
grafts each member's own rows onto its own branch via `ci/ledger-graft.py`, so a
RED batch does not waste the recording.

`--sweep-back` is opt-in, and the reason is a real cost:
`ci/checks/gameplay-falsifiable.toml` carries **no merge driver**, so N branches
appending to it will conflict with each other as they rebase onto main in turn.
The default keeps the rows on the integration tip, where they land once.

That is also a concrete follow-up: **this ledger is a good candidate for
`merge=docregions`.** Its entries are keyed rows — `file = "010-break-block.toml"`
is unique per entry — which is exactly the shape that driver already resolves for
WORKLIST's cold-start index. With it, `--sweep-back` would become safe as a
default and the feasible batch width would rise again.

### 4.2 And on the current queue that saving is zero, for a reason worth fixing

Measured 2026-08-17 with `ci/pack-census.py --branches-from preflight.tsv
--scenario-branches-only`:

> **11 branches bring 60 new gameplay scenarios. All 55 pairs of them collide**,
> so they pack into 11 groups and batching saves **nothing** on the corpus lock —
> 11 recordings serially, 11 batched.

Two files cause all 55, and this is the part that would have been got backwards:

| exempting | pairs freed | groups | recordings saved |
|---|---:|---:|---:|
| `ci/checks/gameplay-falsifiable.toml` alone | 0 | 11 | 0 |
| `ci/checks/gameplay-baseline.toml` alone | 0 | 11 | 0 |
| **both** | **39** | **5** | **6 (~4.0 h)** |

**Neither file alone is worth anything and the pair is worth four hours.** Every
branch that adds a scenario adds a row to both, so removing one blocker leaves
the other holding all 55 pairs. A census that scored single files would have said
"give the ledger a driver", that change would have bought precisely zero, and it
would have read as a fix that did not work.

So the concrete follow-up is **both** files, not one: give
`ci/checks/gameplay-falsifiable.toml` **and** `ci/checks/gameplay-baseline.toml`
a keyed-row merge driver. Both are keyed — `file = "010-break-block.toml"` is
unique per `[[scenario]]` entry — which is the shape `ci/merge-docregions.py`
already resolves for WORKLIST's cold-start index and `ci/figures.toml`'s
`[[figure]]` bodies. That was not done here because
`ci-merge-docregions-registry-tomls` is a queued branch already working in that
registry, and two branches editing it is the collision this whole exercise is
about.

Until then, `ci/batch.sh --sweep` is correct and costs nothing, but on today's
queue it will report one recording per group and save no lock time. The gate
saving in §4 is unaffected — it does not depend on these two files.

`ci/ledger-graft.py` moves entries **verbatim** and refuses everything else: a
requested scenario with no source entry, an existing target entry that differs
(without `--replace`), or a result that does not parse. One bad request refuses
the whole graft rather than writing half of it. That discipline is not fussiness —
the ledger's own header says "do not hand-edit an entry: a hand-written digest is
evidence of nothing". `--selftest` is 16 assertions, and the tool was checked
against the real 108-entry ledger by removing an entry and grafting it back
byte-identically.

## 5. The shared checkout, which is the known failure

On 2026-08-17 a lane merged through `/home/sayer/grit/repo` while a chain held a
branch there. `ci/handoff.sh:418` refuses on `git status --porcelain`, and the
result was 25 minutes lost and **861 consecutive gate refusals**.
`ci/lanes.sh` now refuses outright if its `$ROOT` resolves to that path.

`ci/batch.sh` holds itself to the same rule three ways: it refuses to run if its
own `$ROOT` is the canonical checkout; it refuses an `--int-dir` that is `$ROOT`
or the canonical checkout; and it records the canonical checkout's HEAD, its
checked-out ref and its porcelain count **before any other guard**, re-checking
all three on **every** exit path and printing `SHARED-CHECKOUT` with the verdict.

That last one was an overstatement when first written — the trap was armed near
the bottom of the file, so the early refusals exited without checking anything.
`ci/batch-falsify.sh` caught it. The positive control (`shared_dirty_is_caught`)
dirties the canonical checkout from inside the gate window and asserts the
refusal, so the assertion in the other 25 cases is not a green that cannot go
red.

## 6. Why landing is off, and what would make it safe to switch on

**There is no `git fetch` anywhere in `ci/handoff.sh`, `chain.sh` or
`chain-keeper.sh`** (checked 2026-08-17). Local `main` in the canonical checkout
has exactly one writer today — handoff's own `git merge --ff-only` — and handoff
computes `BEHIND=$(git rev-list --count "$BRANCH..main")` against that **local**
ref. A second writer of `main` therefore produces two failures, neither of which
names its cause:

1. advance `origin/main` only, and every later handoff measures its branch
   against a stale local main, reports `BEHIND=0` when the branch really is
   behind, gates it, and then fails to push as non-fast-forward **after paying a
   full 80-minute gate**;
2. advance local `main` under a worktree that has it checked out, and every file
   of the batch reads as modified there — the 861-refusal bug by another door.

`land_tip` already defends itself: a repo-wide main-writer `flock`, a refusal if
`main` is checked out in **any** worktree, a requirement that local `main`,
`origin/main` **and** the remote's own `ls-remote` all still stand at the gated
base, a compare-and-swap `git update-ref <ref> <new> <old>` rather than a write,
and a rollback on a rejected push. The lease refuses *before* anything is
written, which matters because by the time git rejects a push the local ref has
already moved and only the rollback saves it.

**The checklist:**

1. **`ci/handoff.sh` must take the same mutex around its merge step** (lines
   1059-1062), keyed on
   `$(git rev-parse --git-common-dir)/rustmc-main-writer.lock`. One line either
   side of the ff-merge. Today it is unprotected because it is the only writer;
   batching makes it one of two, and a collision costs the gate that already
   paid 80 minutes.
2. **`ci/handoff.sh` should compare against `origin/main` after a fetch** rather
   than against local `main`. This is the deeper fix and it subsumes failure (1)
   above. Until then, anything that advances main outside handoff must also
   advance the local ref, which is what `land_tip` does.
3. **One measured live batch at width 2 with `--land`**, on two branches whose
   loss would cost nothing, before any width-3 run.
4. **`ci/batch-falsify.sh` green** — 26 cases, 14 injections, 40/40, ~80 s.
5. **`ci/gate-rate.py` must put p above 0.70 at N ≥ 20** over the
   post-preflight population. **SATISFIED 2026-08-18 — see §2.2.** It was the
   one open item, and it was open because the instrument was measuring two
   things it should not have been. `./ci/gate-rate.py --decide` now answers it
   in one command, under every definition at once.

Only then is wiring it into `chain-keeper.sh` a question worth asking, and it is
the owner's to ask. `ci/lanes.sh` is switched off by the same principle.

## 7. Files

| file | what it is |
|---|---|
| `ci/batch.sh` | the batching mode. `--plan` costs seconds; `--land` is the owner's switch |
| `ci/batch-arith.py` | the expected-gate arithmetic. `--cost-green`/`--cost-red` price it in measured minutes instead of gate-units (§2.3); defaults leave every published figure unchanged. `--selftest` is its evidence |
| `ci/batch-falsify.sh` | 26 cases, 14 injections. `BATCH_FALSIFY_DRYRUN=1` checks the patches in 2 s |
| `ci/gate-rate.py` | measures p from every gate log, with its N and its window. `--decide` answers §6 item 5 under every definition; `--per-branch` and `--incidents` are the two corrections in §2.2; `--cost` measures what a gate costs by verdict (§2.3). `--selftest` is its evidence |
| `ci/sweep-tip.sh` | one recording + sweep for this tree, as `role=run`. Refuses when there is nothing to sweep and when a merge gate has the box |
| `ci/ledger-graft.py` | copies named `[[scenario]]` entries between ledgers, verbatim or not at all. `--selftest` is its evidence |
| `ci/pack-census.py` | which files stop branches sharing a tip, and what a driver on them would be worth. Scores combinations, because singles get it backwards |
| `ci/batching-selftests.sh` | all four `--selftest`s plus the injection-patch dry run, in about 3 s |

**None of this is wired into `ci/t0.sh`, deliberately.** That file is the one the
brief names as forcing five of six collision groups, and
`ci-merge-docregions-registry-tomls` is a queued branch already editing that area.
Once it lands, wiring the suite in is one line:

```
./ci/batching-selftests.sh || fail "batching selftests"
```

A note for whoever next touches `ci/batch.sh`: run
`BATCH_FALSIFY_DRYRUN=1 ci/batch-falsify.sh` afterwards. Ten of the fourteen
injection patches were dead on their first run and two were matching the wrong
text and reporting PATCH-OK, which is worse. A patch that has stopped matching
is a hole in the sweep whether or not there are eighty seconds for the cases.
