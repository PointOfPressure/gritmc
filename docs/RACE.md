# The deadline, the win conditions, and the one irreversible rule

**Read this before planning any work.** `docs/WORKLIST.md` says what to do
next; `docs/BENCH.md` holds the evidence. This file says what "done" means,
and states one constraint that cannot be undone once it has been broken.

---

## The constraint that is irreversible — CLEAN ROOM

**Every line of this project is written from vanilla bytecode. Never
transcribe code from any other implementation, in whole or in part — not a
function, not a file, not a struct layout copied out by hand.**

Reading is not the problem and is not forbidden. Techniques and architecture
are not copyrightable, this is a space where other people have solved parts of
it well, and understanding an approach before re-deriving it is ordinary
engineering. Say so in the commit message when an idea came from somewhere
else; attribution costs nothing and keeps the record honest. **Transcription
is the problem, and it is permanent.**

Code taken from a copyleft project stays under that project's licence forever
and cannot be relicensed by us. A CLA can relicense *our* contributors' work
and can never relicense anyone else's. This project's strategy is AGPL + CLA
precisely so a commercial licence stays possible later, and copied code would
carve permanent holes in it — holes found by whoever does the diligence, not
by us, and years after the afternoon they saved.

So the workflow is the one every feature already in this repo used:

```text
unzip oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar
javap -p -c -constants <class>
```

Read the bytecode, port from the bytecode, gate the result against a real
vanilla server. If reading something else told you *what* to build, fine —
then close it and write the code yourself, from the bytecode, and attribute
the idea.

**This is an owner decision and is not revisable by an agent.** That includes
an agent who finds another implementation of this protocol sitting on this
machine, reads the licence, and correctly concludes that copying from it would
be legal and would save a day. It would be, and it would. Do it the long way
regardless: the cost of this rule is hours, and the cost of breaking it is not
recoverable at any price.

---

## What winning means — owner decision, 2026-07-29

**All three of them. Deadline Christmas 2026, tracking the 26.3+ drops as they
land** — a parity claim against a version nobody runs any more is worth
nothing, so version bumps are part of the deadline, not a distraction from it.

1. **Parity — three dimensions, not one.** The Overworld is the measured one.
   The Nether generates and is measured. The End's island density function is
   ported and wired but has no biome or feature data behind it. *Measure
   before porting:* an unmeasured dimension is a guess, not a work item, and
   the first honest number for a dimension is usually much lower than the
   guess was.
2. **Speed — beat Paper, on `ci/bench-servers.sh`, published with the method.**
   Not "beat vanilla", which is a low bar, and not a ratio quoted without the
   machine it was taken on. The harness already runs four flavours back to
   back on one box; the claim is only worth making if the method ships with
   it and someone else can re-take the reading.
3. **Playable — a survival server.** Mobs and AI, redstone, crafting, combat,
   loot. This is the largest scope of the three and the one most likely to be
   deferred forever in favour of another parity decimal place.

Every increment should be traceable to one of those three. If it is not, it is
probably the residual-chasing this project has already been told to stop.

---

## Measure before concluding — including about this project

**Measurements move suspects; readings of code do not.** That is a standing
rule in WORKLIST because it has paid out repeatedly against bugs. It applies
just as much to judgements *about the project itself*, and that is the case
where it is easiest to forget.

It was nearly missed once, expensively. An early assessment of whether this
deadline was reachable at all compared **accumulated state** — how much exists
in this tree against how much exists in mature ones — and concluded it was
not. Nobody had measured **rate**: commits landed per day, features closed per
week, how fast the gap was actually moving. The two readings pointed in
opposite directions, and only one of them had been taken. **A comparison of
state is not a forecast.** Before concluding anything about where this project
stands, name the quantity, take the reading, and write the denominator down.

The same discipline binds inside a benchmark, where it has already been
broken:

* **Say which unit a claim is denominated in.** Wall-clock, CPU-seconds and
  cycles diverge the moment the clock does, and all-core boost means the clock
  always does. A conclusion drawn in cycles and applied to CPU-seconds is a
  different conclusion.
* **A mechanism that explains a number is not the mechanism until a
  measurement separates it from the alternatives.** One such explanation was
  published here and retracted the same day, by one cheap run that had already
  been named as the next step (`docs/BENCH.md`). The retraction cost less than
  the delay would have.
* **Publish the box with the figure.** Cache topology, core count and boost
  behaviour differ enough between machines that a bare ratio does not transfer
  in either direction, and quoting one as if it does is how a benchmark
  becomes marketing.
* **Suspect the measurement when it clears every candidate** — and equally
  when it confirms the first one.

---

## The real risks — none of them is velocity

* **Aim, not speed.** At this commit rate, a week pointed at the wrong thing
  is a week of real work lost. That is why WORKLIST's NOW block must name the
  single next item *and* the result that closes it.
* **Correctness debt.** Moving fast is how a gate ends up green because it
  stopped looking. That has already happened twice here — and the check this
  line used to name, `skipping: no vanilla`, was itself the third: it returns 0
  on every run whether the gate compared anything or not. See
  `docs/PARITY-ROADMAP.md` §73. A green `ci/check-fixture-skips.py` inside T0
  is what says it now, and unlike a grep it can go red.
* **Budget.** ~£38/week of compute, and it drops when school starts in
  September.
* **September.** Time collapses when term begins. Bank the hard, expensive,
  machine-hungry work while there is summer left.
