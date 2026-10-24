#!/usr/bin/env python3
"""Adjudicate every published figure against the artefact that produced it.

# WHY THIS EXISTS

This project's figures drift away from the artefacts that produced them, and
every instance so far was caught by accident. In two days:

* two published figures were refuted by measurement;
* a dashboard verdict compared an **r=7** measurement against an **r=32**
  target, across a gap `docs/BENCH.md` names as uncomparable one paragraph
  earlier;
* a worldgen accuracy percentage turned out to be unquotable because the
  residual had fallen **below** the twin-masking floor;
* a 16-block-margin *suspect list* of 11,623 was quoted as a defect count of
  11,623, when the defect count is 24;
* a margin stated as 19% was 26.6% against the artefact.

Nothing in the tree could have caught any of them, because nothing tied a
number in a document to the thing that produced it. `ci/figures.toml` is that
tie and this script is the adjudication.

# THE DEFECT CLASS THIS MUST NOT JOIN

"A check that could not fail" has now been found twenty-one times here. A
ledger-driven checker is unusually easy to write in that shape: register
nothing and it passes, register only figures you already verified and it
passes forever, add a rule no entry exercises and the rule is decoration.

So this script asserts against **itself** as well as against the tree:

* `[meta] min_figures` — a floor on how much is registered, so emptying the
  ledger is red rather than green;
* `[meta] min_unquotable`, `min_compare_pairs` — every rule below must be
  exercised by at least that many live entries. **A rule no entry exercises
  is not a rule, and this script calls that red.** That is the direct answer
  to the defect class: the rule 6 path and the window-match path cannot be
  silently unreachable;
* the coverage scan — a new percentage in a claim document that nobody
  registered is red, so the ledger cannot rot by omission while the docs grow.
  **AND THE SCAN'S OWN MEMBERSHIP IS THE HOLE IT HAD — 2026-08-17.** The list
  was four hand-kept filenames, so a document that published an exactness
  percentage without being on it was scanned by nothing at all, and nothing
  said so. Measured rather than assumed: `docs/AGENT-BRIEF.md` published six,
  `docs/PARITY-ROADMAP.md` one, `docs/PRODUCT.md` one, and six
  `docs/cold-start/*.md` one or two each — twelve unscanned documents. All of
  their numbers happened to be registered already, so the widening cost
  nothing, which is the argument for doing it before one of them is wrong
  rather than after. `coverage.doc_globs` carries the cold-start directory
  because it gains a file per branch and a hand-kept list of it would go stale
  on the next agent; a glob matching **nothing** is itself red, because a scan
  over an empty set is the `min_external_artefacts` fault in another costume.

# WHAT IT CAUGHT ON ITS FIRST GATE, WHICH IS THE POINT

On 2026-08-07, hours after being written, this check reddened a merge gate on
its own branch. `gate-command-coverage` had landed on `main` while the branch
sat unmerged: it found that `ci/t1-allowlist.py` floors *binaries* rather than
*tests*, so a failing test silenced with `#[ignore]` took T1 from RED to GREEN.
The fix raised `min_binaries` 97 -> 106 and added `min_tests` and
`max_ignored`. The document and the artefact moved together and correctly; the
**ledger** was the stale side, and the check said so in two lines.

Three things came out of that and are now rules 7 and 8 and a new `kind`:

* **A gate floor is a published figure.** It is the number that decides
  whether a branch lands. `kind = "gate-floor"` exists for it, its `artefact`
  is the *adjudicator that reads it*, and rule 8 requires every declared floor
  in `ci/checks/*.toml` to be registered — because the floor that moved was
  caught only by accident of being registered, while the two brand-new ones
  would have gone unregistered indefinitely.
* **`docs/CI.md` deleted its binary counts rather than restating them**, on the
  grounds that the number moves whenever a crate is added and the enforced
  floor is the real figure. That is the correct resolution of a drifting
  number and it is recorded in the ledger's retired list, not mourned.
* **One ledger entry was wrong from the day it was written.** `t1-binaries`
  carried `kind = "bench"` and a window of `view_distance = "n/a"` on a
  test-binary count, because `bench` was the nearest available shape and the
  window had to be filled with something. A window field holding a placeholder
  is the same defect as a missing window wearing a costume, so
  `WINDOW-PLACEHOLDER` now refuses one — while deliberately permitting `none`
  and `unrecorded`, which are real answers.

# THE EIGHT RULES

1. **DOC-DRIFT** — `claim` must appear verbatim in `doc`. Editing the number
   in the document without editing the ledger is red.
2. **ARTEFACT-DRIFT** — `artefact` must exist and contain `artefact_claim`.
   The artefact moving on without the doc following is red. This is the half
   that catches a *measurement* that changed rather than a *sentence*.
3. **ARITHMETIC** — `percent` must equal `100 * (1 - residual/denominator)` at
   the precision it is stated to. An exactness percentage in this project is
   always a derived quantity, so it can always be recomputed, and a typo or an
   invented digit is red.
4. **RULE 6 (resolving power)** — `quotable = true` with `floor >= residual`
   is red: a residual under the vanilla-A-vs-vanilla-B floor cannot support an
   accuracy claim. And `quotable = false` is not enough on its own — the
   document must *print* the `disqualifier` beside the number, so a reader who
   never opens the ledger still cannot lift the figure out.
5. **WINDOW-MATCH** — an entry with `compare_to` must agree with its target on
   every key in `compare_keys`. This is the r=7-against-an-r=32-target
   mistake, expressed mechanically.
6. **BASELINE** — every figure declares what it was measured against. Anything
   that is not the pinned unmodified vanilla jar must carry `baseline_note`
   AND that note must be visible in the document. The launch position is that
   this project's parity is measured against **unmodified** vanilla; a figure
   that quietly changed baseline would dissolve it, so the label is enforced
   rather than trusted.
7. **WINDOW-PLACEHOLDER** — a window field holding `n/a`, `tbd`, `unknown` or
   similar is red. `none` and `unrecorded` are permitted because they are real
   answers; a token meaning "this field does not apply here" is a sign the
   entry is wearing the wrong `kind`.
8. **UNREGISTERED-FLOOR** — every `min_*`/`max_*` in `ci/checks/*.toml` must be
   a registered `gate-floor` figure. A floor left in place after the code that
   enforced it was deleted is a check that cannot fail, wearing a number.

# THE FAULT THE FIRST EIGHT RULES COULD NOT SEE — 2026-08-13

Every rule above adjudicates a figure against **its own** artefact, doc and
window. None of them asks whether the *provenance record already in the entry*
supports the word `quotable`, and so `quotable = true` stayed a thing a human
typed. It was wrong in the ledger for three days:
`readme-restates-ow-r16-admissible` carried `quotable = true` while the entry
it restates, `ow-r16-admissible-n2`, carried
`quotable = false, disqualifier = "uncontrolled chunk order"`. The same number,
the same denominator, the same references — quotable in one row of this file
and withdrawn in the other. `README.md`'s prose was correct throughout and
said **not quotable** and **uncontrolled chunk order** in as many words; only
the machine-readable half disagreed, which is the half a consumer enumerating
this ledger would believe.

The reason it survived rule 5 is visible in the entry: its `compare_keys` list
omitted `dump_order`, the single key its source was disqualified on. So the
window-match rule compared the two rows on every axis except the one that had
already been ruled fatal, and reported agreement.

Rules 9-12 close that, and they share one premise: **an entry's quotability is
a claim about its provenance, so it is checkable against the provenance the
entry already records.** No new hand-typed field carries the answer — that
would only move the typing.

9. **RESTATED-QUOTABILITY** — an entry that *republishes* its `compare_to`
   target's number (identical `percent`, or identical `residual` over an
   identical `denominator`) may not be `quotable = true` while that target is
   `quotable = false`. A restatement can be faithful or unfaithful; it cannot
   out-rank the measurement it restates. Numeric identity is what separates a
   restatement from a comparison, and the distinction is load-bearing rather
   than fussy: `use-table-probe-moves-20260811` compares against a target
   disqualified as an `upper bound` and is rightly unaffected, because it
   publishes a different quantity measured on the same window.
10. **RESTATED-WINDOW** — for that same republishing relation, `compare_keys`
    must cover **every** window key the target records, and the restating
    entry must itself record every one of them. If it is the same number it is
    the same measurement, and there is no key on which it may legitimately
    differ. This is the half that catches the *reason* rather than the
    symptom, and it is deliberately not extended to a comparison: comparing
    across a subset of keys is the normal and correct use of `compare_to`.
11. **FLOOR-UNDECLARED** — a `quotable = true` entry of a kind rule 6 applies
    to (`FLOOR_APPLIES` below) must carry either a `floor`, so that rule 4 can
    fire on it, or a `floor_note` saying why no floor exists. Silence is red.
    Before this rule, an exactness-shaped figure could be quotable with rule 4
    structurally unable to reach it, and nothing distinguished "measured
    against its floor and clear of it" from "never faced one".
12. **DUMP-ORDER-UNRECORDED** — a ratchet, not an absolute. 26 of the 52
    chunk-order-sensitive entries in this ledger omit `dump_order`, eleven of
    them quotable, and filling those in is a measurement rather than a
    bookkeeping edit — see `[meta] max_quotable_without_dump_order`. What is
    enforced is that the count may not **rise**. `docs/DETERMINISM.md` §15
    prices a wrong chunk order at 117,628 blocks on a candidate that is
    unmodified vanilla, so a new quotable worldgen figure that does not say
    which order its dump walked is not a weaker claim than one that does, it
    is an unconditioned one.
13. **VERDICT-MARKER** — a `quotable = false` figure's claim may not be printed
    on a line carrying the ✅ these documents use for "settled". Rule 4 is
    satisfied by the disqualifier appearing anywhere within 4,000 characters,
    and on 2026-08-13 it was: `docs/PARITY.md` withdrew 99.972345% in one
    paragraph and opened another, twenty lines later, with
    "✅ **If only one *number* fits, it is 99.972345% ...**". **Proximity is
    not attachment.** The marker vocabulary already exists and is used — that
    same file writes `99.972345% 🚫 uncontrolled chunk order` in its own table
    — so this binds an existing convention rather than inventing one. Found by
    auditing the whole ledger after rules 9-12 closed the entry that motivated
    them, which is the case for auditing: the same fault, in the document
    rather than in the flag, and pointing the other way.

# THE FAULT RULES 1-13 COULD NOT SEE — 2026-08-17

Every rule above, rules 4 and 13 included, reads `texts[fig["doc"]]` and nothing
else. **A withdrawn figure republished in any OTHER tracked document is
invisible to all of them.** That is how `docs/WORKLIST.md` line 7890 called the
withdrawn 29,605 "the quotable figure" for a week, and how `docs/AGENT-BRIEF.md`
— the one file every agent reads first and no agent re-checks — came to carry
four withdrawn claims. The ledger's own `ow-r16-admissible-n2` provenance names
`docs/WORKLIST.md` as a place the number is published, while no rule looked
there.

Measured across all 72 scanned documents before choosing a shape: **65 mentions
of a withdrawn claim outside its own doc, 0 on an endorsement line, 24
occurrences with no disqualifier within 4,000 characters.** Two counts, two
rules, deliberately not merged into one:

14. **ECHO-VERDICT-MARKER** — absolute, and it is rule 13 with the
    own-document restriction removed. Zero violations today and it must stay
    zero: printing a withdrawn number on a line that says the work is settled is
    never right, wherever it happens. A rule with no live violation is this
    script's own warned-about shape, so it is held by
    `ci/check-figures-falsify.py` injecting one rather than by the absence of
    one.
15. **ECHO-UNQUALIFIED** — a ratchet at `[meta] max_unqualified_echoes`. The 24
    are not all defects: a percentage recurring inside a table, or a document
    that states the withdrawal in its own words rather than in the ledger's
    `disqualifier` wording, both land here, and auditing them is a reading job
    across four claim documents. What is enforced is that the count may not
    **rise** — every one of the 24 arrived by somebody adding a mention with
    nothing objecting.

# RULE 4's *PASS* WAS AS DANGEROUS AS ITS FAIL — 2026-08-17

Rule 4 asks `floor >= residual` and says nothing about whether the two numbers
were produced by the same instrument. On `ow-r16-admissible-n2` they were not:
the residual 29,605 is a charge under a rule that **excludes nothing**
(`mask = "none"`), and the floor 29,560 is the **width of the twin mask** that
rule does not use. So rule 4 read `29,560 < 29,605`, declined to disqualify, and
reported the difference as resolving power — **that is the "45 blocks of
headroom" `docs/DETERMINISM.md` §14 killed on 2026-08-09, generated mechanically
by this checker rather than typed by a human.** Flipping that entry to
`quotable = true` on the pre-2026-08-17 checker is GREEN, exit 0.

16. **FLOOR-NOT-COMPARABLE** — `floor_comparable = false` marks a floor whose
    instrument differs from its residual's, and it must carry `floor_basis`
    naming what the floor actually measures. The flag is two refusals, not an
    allowance: rule 4's arithmetic is **skipped** (a comparison nobody should
    draw is not one to pass), and `quotable = true` is red outright, because a
    level cannot rest on a floor its own residual cannot be compared to.

Measured before adding it, so this is a two-entry annotation rather than a
ledger-wide migration: 22 figures carry a `floor` under `mask = "none"` and
exactly **two** take that floor from a different instrument —
`ow-r16-admissible-n2` and its restatement. The other twenty carry an
innocent-candidate floor from their own instrument, which is the comparison rule
4 was written for.

# THE TWO RULES THE BENCH AND PROVENANCE AUDIT ADDED — 2026-08-18

17. **BENCH-WORKERS-UNRECORDED** — a ratchet, rule 12's shape on the bench
    kinds. Every `bench` and `bench-target` entry must carry `workers`,
    whichever arm of the bench window it satisfies, and the count of *quotable*
    ones still saying `"unrecorded"` may not rise — see
    `[meta] max_quotable_bench_without_workers`. The bench window disjunction
    constrains the square or the server and stops there; it never reached the
    pool, and CPU-seconds is a function of the pool.
    `pipeline::worker_threads`'s own doc prices the same generator, seed,
    square and build at 78.54 CPU-s on one thread, 82.69 on twelve and 98.48 on
    twenty-four — 25.4%, against a claim published to six significant figures.
    `ci/bench-servers.sh` set `RUSTMC_WORKERS` nowhere until 2026-08-18, so any
    shell that had exported it moved every rustmc row and left no trace in
    `results.tsv`, whose `peak_cores` column is an observation and not the
    setting. `"unrecorded"` is an honest answer and is why this is a ratchet
    rather than a ban: the eight entries holding it cannot be repaired by
    editing this file, only by re-taking the run with the script that now
    stamps the width.

18. **PROVENANCE-NO-COMMIT** — `provenance` must name a commit that resolves in
    this repository. Twenty-five entries carried a bare date, and a figure whose
    baseline tree is unrecorded cannot be the *before* of an attribution: the
    stale-tenth drift in `docs/BENCH.md` 3 could only be attributed because
    `ed0a29ec` had been written down somewhere outside this file. A date is not
    merely vaguer than a sha, it can be wrong — `ow-r16-n2-prechambers` was
    recorded as 2026-08-03 and its commit landed at 00:29 on the 4th, one
    sitting either side of midnight.
    **The rule asks git rather than matching a pattern**, and that is the half
    that earns its keep: `20260812` is a date wearing hex's costume and appears
    inside four live provenances, while a mistyped sha is well-formed and reads
    exactly like a copied one. Provenance is prose here — branches, dates,
    machines and a sha in any order — so every hex-shaped run is a candidate and
    the entry passes if any one of them resolves. Resolution is skipped where
    git cannot answer, and the rule degrades to a shape check rather than
    reddening for a reason unrelated to the ledger.

19. **DASHBOARD-UNREGISTERED** — a ratchet, and it closes a hole in rules 1-8
    rather than adding a new axis. `[coverage] pattern` is
    `[0-9]+[.][0-9]{4,}%` and `docs/PARITY.md` 2 is made entirely of counts, so
    the UNREGISTERED scan that guards every percentage across seven documents
    could not see one number in the table a reader actually lands on. Five of
    its twelve residuals were in no ledger entry. Two of those five are the
    RAW halves of rows whose order-invariant cores are fully registered with a
    window and a floor — and one of them, the nether's 39,941, is 2.4x under
    its own floor and was explicitly withdrawn in favour of its core, while the
    dashboard went on printing it in the same row at the same weight.
    A row reading `not measured` is not counted: that is an honest statement of
    absence and is the distinction the table had no column for until the floor
    column was added. This rule reads the file from disk rather than from the
    cited-documents map, because a dashboard nothing cites is exactly the case
    it exists for.

20. **STAMP-DISAGREES** — an entry whose `artefact_claim` carries a
    `corpus=N` stamp must agree with it in `window.scenarios`. Rule 2 asserts
    the stamp is still present in the artefact and stops there, so an entry
    could cite one run and describe another, and eleven did.
    `docs/PARITY-ROADMAP.md` 4479 states the correction outright — "The
    denominator is 92, and every document that led here said 93" — and
    `ci/checks/gameplay-baseline.toml` carries the arithmetic behind it. The
    correction reached the documents and not this file: five entries said 93
    against a stamp of 92, six said 93 against a stamp of 106. **The stamp
    wins.** It is what the run printed about itself; the window field is what
    someone typed afterwards from a brief, and this project's briefs were the
    thing that was wrong.

Exit 0 green, 1 red, 2 the ledger or the tree is malformed.
"""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - toolchain floor
    print("check-figures: needs python 3.11 for tomllib", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "ci" / "figures.toml"

# A figure is only meaningful with its window. These are the fields each kind
# must carry; an entry that omits one is malformed, not merely unverified, so
# "I did not record the window" cannot be expressed in the ledger.
#
# **`harvest` is required for the same reason `refs` is, and it was added on
# 2026-08-10 after the cost of its absence was measured.** The chunk order the
# references were harvested in is a condition of the comparison in the way the
# seed is, not a detail of how they were made: `docs/DETERMINISM.md` §14 puts
# 146,169 blocks between two unmodified vanilla runs of the same seed and
# window harvested in different chunk orders, against 32,127 between two runs
# of the same order — a factor of 4.6. The published r=16 overworld level was
# taken with our dump walking x-major against references harvested z-major and
# nothing in this ledger recorded that, so the figure read as a residual when
# 11,820 blocks of it were a condition nobody had set. `"unrecorded"` is an
# honest and accepted value; a missing field is not.
REQUIRED_WINDOW = {
    "worldgen": ["dimension", "seed", "radius", "centre", "chunks", "refs", "harvest", "mask"],
    "worldgen-floor": ["dimension", "seed", "radius", "centre", "chunks", "refs", "harvest", "mask"],
    "vanilla-self": ["dimension", "seed", "radius", "centre", "chunks", "refs", "harvest", "mask"],
    # `bench` is the only kind with an empty list here, because its window is a
    # DISJUNCTION and not a set: radius+chunks (a generation square) or
    # level+view_distance (a running server). It is enforced in `check_one`
    # below rather than by this table.
    #
    # An empty list beside a prose promise is the shape of an unfalsifiable
    # check, so on 2026-08-18 the promise was tested rather than trusted:
    # `ci/check-figures-falsify.py` injects a bench entry with neither arm,
    # with half the generation arm and with half the running-server arm, and
    # asserts WINDOW-INCOMPLETE on all three — plus a green control on the
    # running-server arm alone, which is what `boot-superflat` and the three
    # `rss-*` entries actually look like. The promise holds.
    #
    # What is NOT promised, and what testing it did not fix: the disjunction
    # constrains the SQUARE and nothing else. A bench figure was not required
    # to name the commit it was taken on, nor the worker-pool size it ran
    # against, and neither is derivable from the other fields. Both are now
    # required — see BENCH_ALWAYS below.
    "bench": [],
    "bench-target": ["radius", "chunks"],  # plus BENCH_ALWAYS
    # A floor enforced by a gate is a published figure too, and it is the kind
    # that rots most quietly: nobody reads it until it fails to fire. Its
    # window is not a world, it is the run that set it — which command, on
    # which tree, when. Added 2026-08-07 after `min_binaries` went 97 -> 106
    # on `main` and this ledger was the stale side.
    "gate-floor": ["command", "tree", "measured"],
    # ORACLE rule 6, in the gameplay corpus. Added 2026-08-09 with
    # `ci/corpus-floor.sh`, and the window is the shape that makes the number
    # mean anything: which ensemble of vanilla recordings, how many of them,
    # how many pairwise comparisons, over how many scenarios, at what alpha.
    #
    # **`runs` and `alpha` are required for the same reason `refs` is required
    # of a worldgen figure, and the lesson is the same one.** A corpus floor is
    # a LOWER bound that grows with N — `docs/PARITY.md` §4 records the End's
    # floor as 0 across two references and 252 across six — so a floor quoted
    # without the N that produced it is not a weaker claim than one quoted with
    # it, it is a different and false claim.
    "corpus-floor": ["ensemble", "runs", "pairs", "scenarios", "alpha"],
    # Block entities, added 2026-08-16 with `ci/be-census-12345.sh`
    # (docs/DETERMINISM.md 40). A new kind rather than a `worldgen` one
    # because THE DENOMINATOR IS NOT BLOCKS: 173 block entities sit inside a
    # window of 107,053,056 block states, and letting the two share a kind is
    # how a count of 173 ends up being read against a denominator of a
    # hundred million. `unit` is required for exactly that reason and must say
    # which thing is being counted. `mask` is NOT required here and its
    # absence is deliberate — nothing in this instrument masks anything, and a
    # required `mask = "none"` would imply a masked variant exists.
    "block-entity": ["dimension", "seed", "radius", "centre", "chunks", "refs", "harvest", "unit"],
    "block-entity-floor": ["dimension", "seed", "radius", "centre", "chunks", "refs", "harvest", "unit"],
    # How much of VANILLA the corpus reaches, rather than how much of the
    # corpus we pass. Added 2026-08-11 with `ci/vanilla-reach-census.py`
    # (docs/PARITY-ROADMAP.md 21). Its window is the shape that makes the
    # number mean anything, and `unit` is in it because the whole finding of
    # that section is that the answer moves 55x with the choice of unit: a
    # reach figure without its unit is not a weaker claim, it is a different
    # one. `denominator_source` is required for the opposite reason — the
    # entire value of these figures is that the denominator is Mojang's and
    # not ours, and an entry that cannot say where its denominator came from
    # has given that up.
    "corpus-reach": ["unit", "denominator_source", "ensemble", "runs", "scenarios", "jar"],
    # A property of a GENERATED GOLDEN TABLE rather than of a world or a corpus.
    # Added 2026-08-11 with `ci/probe-reads.py` and `UseGolden.STANCES`
    # (docs/PARITY-ROADMAP.md 22). Its window is the three things that decide
    # what the number means: the `jar` it was read from, the `generator` whose
    # probe produced the table, and `stances` — how many players the sweep
    # actually turned. `stances` is required for the same reason `runs` is
    # required of a corpus floor: "constant across all probe states" over one
    # stance and over eleven are different claims, and the first is not a weaker
    # version of the second, it is a false one.
    "table-audit": ["unit", "denominator_source", "jar", "generator", "stances"],
    # A COMPOSITE over several `corpus-reach` units, times a corpus score.
    # Added 2026-08-12 with `ci/parity-aggregate.py` (docs/PARITY-ROADMAP.md
    # 37). Its window carries `estimator` for exactly the reason `corpus-reach`
    # carries `unit`: the whole finding of that section is that the answer
    # moves with a choice nobody is forced to disclose — median 10.16%, mean
    # 11.43%, pooled 12.53% on one set of inputs — so an aggregate quoted
    # without its estimator is not a weaker claim, it is a different one.
    #
    # `units` is required because a composite over four rows and a composite
    # over seven are not the same figure, and three registry rows are excluded
    # today for want of a ceiling. `projection` is required because the match
    # half is a count under a comparator and `oracle-runner compare --json`
    # records none, so the identity exists only where someone wrote it down.
    "parity-aggregate": ["estimator", "units", "basis", "ensemble", "runs",
                         "scenarios", "projection", "jar"],
    # A figure this tree PUBLISHES AND DID NOT MEASURE. Added 2026-08-18 with
    # `ci/checks/cited-audits.toml` and `ci/check-cited-audits.py`, which pin
    # each one to the commit it was taken on and re-read that blob every T0.
    #
    # `unit` is required for the same reason `corpus-reach` requires it, and the
    # server audit makes the case in its sharpest form: the same surface is 47
    # gaps, 85 markers or 29 files, and only the first is a quantity anyone can
    # work from. A count without its unit is not vaguer, it is a different
    # number.
    #
    # `bound` is required and is the field this kind exists for. A cited figure
    # is read second-hand, so the one thing a reader cannot recover for
    # themselves is whether it is exact, a floor or a ceiling — and here it is
    # usually a floor: 47 is what `rustmc-server`'s own refusal vocabulary
    # catches, and `item.rs`'s `DEFAULT_MAX_STACK_SIZE` is a fully written-up
    # extraction gap that no marker regex reaches because it says "a named gap"
    # rather than "unported". Published bare, a lower bound reads as a census.
    #
    # `branch` is where it was measured and `basis` is what it was derived
    # from; between them a reader can go and check it without this file.
    "cited-audit": ["unit", "denominator_source", "basis", "branch", "bound"],
}

UNMODIFIED = "unmodified"

# A window field filled with a placeholder is not a window field. This is the
# same failure as omitting it, wearing a costume, and the ledger had one from
# the day it was written: `t1-binaries` carried `view_distance = "n/a"` on a
# test-binary count, because `bench` was the nearest kind and its window shape
# had to be satisfied somehow. `gate-floor` exists so that entry has a shape
# that fits; this list is what stops the next one being fudged.
#
# Two words are deliberately NOT here, and the first cost a red on the run
# that introduced this rule:
#
# * **`none`** is a real answer. `mask = "none"` means no twin mask was
#   applied, which is exactly what makes a floor row a floor row. Banning it
#   would have forced seven honest entries to lie.
# * **`unrecorded`** is a real answer too — the End's older reference genuinely
#   has no recorded harvest order, and that is *why* the figure derived from it
#   is unquotable. Banning it would push a true "we do not know" back into a
#   placeholder, which is the opposite of the point.
#
# What is banned is a token that means "this field does not apply here", which
# is a sign the entry is wearing the wrong `kind`.
PLACEHOLDERS = {"n/a", "na", "-", "", "unknown", "tbd", "?", "todo"}

# The verdict marker these claim documents already use for "this one is
# settled". `docs/PARITY.md` carries 6 of them against 9 `🚫`, and
# `docs/WIDE-WINDOW-R16-ALL-DIMENSIONS.md` 3 against 4, so it is an existing
# convention rather than one this checker invents. Rule 13 makes it binding.
ENDORSEMENT = "\u2705"

# How close a disqualifier has to sit to count as printed beside a number. Rule
# 4 has used 4,000 characters since it was written; the echo rules below use the
# same window so the two answers cannot disagree about what "beside" means.
NEAR = 4000


def check_echoes(figures: list[dict], texts: dict[str, str]) -> tuple[list["Red"], int]:
    """Rules 4 and 13, applied OUTSIDE each figure's own registered `doc`.

    Returns (reds, unqualified_count). The verdict-marker half is absolute and
    comes back as reds; the unqualified half is a ratchet, so it comes back as a
    count for the caller to compare against its ceiling.
    """
    reds: list[Red] = []
    unqualified = 0
    for fig in figures:
        if fig.get("quotable") is not False:
            continue
        claim = fig.get("claim")
        if not claim:
            continue
        fid = fig.get("id", "?")
        dq = str(fig.get("disqualifier", "") or "")
        for rel, text in sorted(texts.items()):
            if rel == fig.get("doc") or claim not in text:
                continue
            for line in text.splitlines():
                if claim in line and ENDORSEMENT in line:
                    reds.append(
                        Red(
                            "ECHO-VERDICT-MARKER",
                            fid,
                            f"`{claim}` is not quotable ({dq or 'withdrawn'}) and "
                            f"is printed in {rel} \u2014 which is NOT this figure's "
                            f"registered doc ({fig.get('doc')}) \u2014 on a line marked "
                            f"{ENDORSEMENT}: {line.strip()[:120]!r}. Rules 4 and 13 "
                            "read only the registered doc, so this line was "
                            "invisible to both.",
                        )
                    )
            if not dq:
                continue
            start = 0
            while True:
                idx = text.find(claim, start)
                if idx < 0:
                    break
                start = idx + len(claim)
                window = text[max(0, idx - NEAR) : min(len(text), idx + NEAR)]
                if dq.lower() not in window.lower():
                    unqualified += 1
    return reds, unqualified

# The kinds ORACLE rule 6 has anything to say about — a residual that could in
# principle sit under a vanilla-against-itself floor.
#
# The exclusions are by construction and not by convenience. `worldgen-floor`,
# `vanilla-self` and `corpus-floor` entries **are** floors: a floor has no
# floor, and demanding one would be asking what vanilla's irreproducibility is
# reproducible against. `bench`/`bench-target` are CPU-seconds, wall-clock and
# RSS, which have run-to-run spread but no reference ensemble. `gate-floor` is
# a merge-blocking count.
FLOOR_APPLIES = {"worldgen", "corpus-reach", "table-audit"}

# The kinds whose comparison is conditioned on the chunk order the dump walked.
# `harvest` (the reference's order) became a required window key on 2026-08-10;
# `dump_order` is the other half of the same pair and is the one that was
# actually uncontrolled. See rule 12 and `[meta] max_quotable_without_dump_order`.
CHUNK_ORDER_KINDS = {"worldgen", "worldgen-floor", "vanilla-self"}

# The window keys EVERY bench figure carries, on both arms of the disjunction.
#
# The disjunction above constrains the square or the server, and nothing else.
# It does not reach the worker pool, and the worker pool is what a CPU-seconds
# figure is a function of: `pipeline::worker_threads`'s own doc prices the same
# generator, seed, square and build at 78.54 CPU-s on one thread, 82.69 on
# twelve and 98.48 on twenty-four — a 25.4% span, the same 810 G instructions
# to within 0.2% arriving as 383 G cycles instead of 325 G.
#
# `ci/bench-servers.sh` set `RUSTMC_WORKERS` nowhere until 2026-08-18, so the
# pool came from the box's physical core count unless the invoking shell had
# exported it, in which case every rustmc row moved and no artefact recorded
# it — `results.tsv` has flavour, centre, chunks, wall, CPU, peak cores and
# spawn ms, and `peak_cores` is an observation rather than the setting (a pool
# of 24 that peaked at 11 writes what a pool of 12 that peaked at 11 writes).
#
# This is `harvest`'s lesson on a different axis and it gets `harvest`'s
# treatment: `"unrecorded"` is an honest and accepted value, a missing field is
# not, and the count of quotable entries still saying `"unrecorded"` is
# ratcheted so it can only fall.
BENCH_ALWAYS = ["workers"]

# A provenance must name a COMMIT. Seven to forty hex characters, anywhere in
# the string, and `git` must recognise it as an object in this repository.
#
# Twenty-five entries carried a bare date until 2026-08-18, and a date does not
# identify a tree — the bench-regression attribution could only proceed because
# `ed0a29ec` had been written down somewhere else. Worse than useless at the
# boundary: `ow-r16-n2-prechambers` was recorded as 2026-08-03 and its commit
# landed at 00:29 on the 4th, so the recorded date and the true tree disagree by
# a calendar day for a measurement taken in one sitting.
#
# The shape check and the resolution check are separate on purpose. Shape is
# what a hermetic fixture can test; resolution is what catches the real failure,
# which is a sha that was typed rather than copied. Resolution is skipped where
# `git` cannot answer — off a checkout, in a fixture — so the rule degrades to
# the shape check instead of going red for the wrong reason.
PROVENANCE_SHA = re.compile(r"\b[0-9a-f]{7,40}\b")
BENCH_KINDS = {"bench", "bench-target"}


def republishes(fig: dict, other: dict) -> bool:
    """Does `fig` publish the same NUMBER as `other`, rather than compare with it?

    This is the restatement test rules 9 and 10 turn on, and it is numeric on
    purpose. The tempting test — an identical `claim` string — is wrong here,
    and the ledger already holds the counterexample that proves it:
    `use-table-probe-moves-20260812` restates the *sentence* ``17 of 184 rows
    move when the player does`` from `use-table-probe-moves-20260811` and is
    nevertheless an independent re-measurement, taken from a different
    generator sha against a different artefact stamp, which deliberately drops
    `generator` from its `compare_keys` because the generator is the thing
    that changed. Treating it as a restatement would demand it compare on the
    one key its whole finding is about.

    Two entries that publish the same derived quantity over the same
    denominator, on the other hand, are the same measurement whatever prose
    surrounds them.
    """
    if fig.get("percent") is not None and fig.get("percent") == other.get("percent"):
        return True
    if fig.get("residual") is not None and (fig.get("residual"), fig.get("denominator")) == (
        other.get("residual"),
        other.get("denominator"),
    ):
        return True
    return False


class Red(Exception):
    """One failed rule, with the rule's name so the output is greppable."""

    def __init__(self, rule: str, fid: str, detail: str) -> None:
        super().__init__(detail)
        self.rule = rule
        self.fid = fid
        self.detail = detail


def read(path: Path) -> str:
    if not path.exists():
        raise Red("ARTEFACT-MISSING", "-", f"{path.relative_to(ROOT)} does not exist")
    return path.read_text(encoding="utf-8", errors="replace")


def pct_to_float(s: str) -> tuple[float, int]:
    """'99.972035%' -> (99.972035, 6 decimal places)."""
    body = s.strip().rstrip("%")
    dec = len(body.split(".")[1]) if "." in body else 0
    return float(body), dec


def sha_resolves(sha: str) -> bool:
    """Is `sha` an object in this repository? Cached — the ledger repeats shas."""
    if sha in _SHA_CACHE:
        return _SHA_CACHE[sha]
    ok = subprocess.run(
        ["git", *_STORE_ARGS, "cat-file", "-e", f"{sha}^{{commit}}"],
        capture_output=True,
    ).returncode == 0
    _SHA_CACHE[sha] = ok
    return ok


_SHA_CACHE: dict[str, bool] = {}

# `_looks_like_a_date` lived here and is deleted with the shape fallback that
# was its only caller — 2026-09-05. It excluded a run that parses as YYYYMMDD so
# that `20260812` could not pass for a sha, which was the correct refinement of
# `not c.isdigit()` after that blunter test refused the real sha `27914806` in
# every rsynced gate directory and cost the merge chain an hour on 2026-08-18.
# Both were answers to the wrong question. Shape can separate a date from a sha
# and cannot separate a sha from a sha, and the case rule 18 exists for is
# `48d1efb` — well-formed, not a date, and naming no object anywhere. A dead
# helper left behind reads as a rule still being applied, which is the shape of
# the `ACCEPT_TELEPORTATION` field this project has already paid for once.

# RESOLVE_SHAS asks `ci/lib/box_env.py` rather than `git -C ROOT`. The date test
# above, from `f82b2ebb`, is what keeps the DEGRADED path honest when there is
# no history at all; this is what stops the path being taken in the first place
# in a gate directory, because `ci/t0-remote.sh` now ships a bare object store
# and `box_env` finds it. A hermetic fixture still has neither and still
# degrades, which is what the fallback is for.

# Resolution needs history. It used to ask only `git -C ROOT`, which is the
# checkout — and every gate in this project runs in a copy shipped with
# `--exclude '.git'`, so the answer was always no. The rule then fell back to
# its shape half, which rejects all-digit candidates because a date wears hex's
# costume, and reddened `ow-r16-12345-admissible-iceberg` on every branch over
# a provenance that is correct. See rule 18 below for what that cost.
#
# `ci/lib/box_env.py` also finds `.gitstore`, the bare store `ci/t0-remote.sh`
# now ships, so in the box copy this resolves for real rather than degrading.
# A hermetic fixture still has neither and still degrades, which is what the
# fallback is for.
sys.path.insert(0, str(ROOT / "ci" / "lib"))
import box_env  # noqa: E402

try:
    _STORE = box_env.git_store(ROOT)
except box_env.BoxEnvRed as _exc:  # a broken store is never a quiet degrade
    print(f"check-figures: {_exc}", file=sys.stderr)
    raise SystemExit(1)
_STORE_ARGS = box_env.store_args(_STORE)
# The date test. It used to guard the shape fallback, which is gone (the
# snapshot replaced it); it now guards candidate selection on the resolving
# path, where a date that prefix-matches some object would otherwise pass as
# a commit. One definition, in box_env, shared with the snapshot writer.
_looks_like_a_date = box_env.looks_like_a_date
RESOLVE_SHAS = _STORE is not None

# ===== THE RULE USED TO PASS BY NOT BEING ABLE TO CHECK — repaired 2026-09-05 =====
#
# `RESOLVE_SHAS` is false in exactly one place and it is the only place that
# matters: **every gate directory on the build box.** `ci/t0-remote.sh` rsyncs
# with `--exclude .git` on purpose, so `git rev-parse --git-dir` fails there and
# rule 18 fell back to its shape half, which asks only "is there a hex-shaped
# run that is not a date". Measured on main `1a31e0a56`, same tree both ways:
#
#     worktree with history   check-figures.py  exit 1, 3 PROVENANCE-NO-COMMIT
#     rsync of it, no .git    check-figures.py  exit 0, FIGURES GREEN, 0 reds
#
# and the three are real — `48d1efb`, `00ab0bde` and `917ebc43` resolve to no
# object in any repository, reflog or branch on this box. So the rule was red
# on main and had been for as long as anyone had shipped it, and every gate
# said green because the gate is where it cannot look.
#
# `ci/check-figures-falsify.py` did not catch this and could not have: its
# fixture `git init`s, so all three of its rule-18 cases exercise the resolving
# arm, and no case had ever run the arm the gate actually takes. A falsifier in
# a different mode from the gate proves the rule can go red somewhere nobody
# runs it.
#
# The repair ships the ANSWER, not the database. `ci/figure-provenance.py
# --snapshot` resolves every provenance in the worktree, which has history, and
# stamps `/.figure-provenance`; `ci/t0-remote.sh` stamps it immediately before
# the rsync and it travels with the tree, the same idiom `/.rp-commit` has used
# since 2026-08-06. The alternative was shipping `.git` — 165 MB, and it would
# give every gate directory a HEAD that disagrees with its own working tree,
# which is the property `--exclude .git` exists for.
#
# So in a `.git`-less tree the rule now REFUSES on an absent or stale snapshot
# rather than degrading. That is deliberate and it is the whole point: "could
# not be checked" was being reported as "checked and fine", and the only way a
# transport can stop hiding a rule is for the missing transport to be the red.
SNAPSHOT = ROOT / ".figure-provenance"
SNAPSHOT_VERSION = "1"


def _load_snapshot() -> tuple[dict[str, tuple[str, str]], str | None]:
    """`{id: (provenance_digest, oid|UNRESOLVED)}`, or an error string.

    Deliberately a copy of `ci/figure-provenance.py`'s reader rather than an
    import: this file is copied alone into `ci/check-figures-falsify.py`'s
    hermetic fixture, and a checker that stops running when a sibling is absent
    is a checker whose falsifier tests something else.
    """
    if not SNAPSHOT.exists():
        return {}, (
            f"{SNAPSHOT.name} is absent. This tree has no `.git`, so rule 18 "
            "cannot resolve a provenance itself and the stamp it reads instead "
            "was never written. `ci/t0-remote.sh` writes it before the rsync; a "
            "gate directory populated some other way needs "
            "`ci/figure-provenance.py --snapshot` run in the worktree first."
        )
    rows: dict[str, tuple[str, str]] = {}
    version = declared = None
    for line in SNAPSHOT.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#") or line.startswith("tree "):
            continue
        if line.startswith("version "):
            version = line.split(None, 1)[1]
            continue
        if line.startswith("figures "):
            declared = line.split(None, 1)[1]
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            return {}, f"{SNAPSHOT.name} has a malformed row: {line[:60]!r}"
        rows[parts[0]] = (parts[1], parts[2])
    if version != SNAPSHOT_VERSION:
        return {}, (
            f"{SNAPSHOT.name} is version {version!r} and this checker reads "
            f"{SNAPSHOT_VERSION!r}"
        )
    if declared is None or int(declared) != len(rows):
        return {}, (
            f"{SNAPSHOT.name} declares {declared} figures and carries {len(rows)}"
        )
    return rows, None


SNAPSHOT_ROWS, SNAPSHOT_ERROR = ({}, None) if RESOLVE_SHAS else _load_snapshot()


def check_one(fig: dict, by_id: dict, texts: dict) -> list[Red]:
    fid = fig.get("id", "<no id>")
    reds: list[Red] = []

    # `unit` and `question` joined this list on 2026-08-19. Six of the previous
    # run's corrections were one class — a right number answering a question
    # nobody asked — and every one of them was arithmetically correct with no
    # field in this ledger disagreeing with any other, because the ledger
    # recorded what each number IS and never what it ANSWERS. Required, not
    # optional: an optional field defaults to absent and a checker that skips
    # the entries missing it measures its own input.
    for field in ("id", "kind", "doc", "claim", "quotable", "baseline", "artefact",
                  "unit", "question"):
        if field not in fig:
            reds.append(Red("MALFORMED", fid, f"missing required field `{field}`"))
    if reds:
        return reds

    kind = fig["kind"]
    window = fig.get("window", {})

    # -- window completeness -------------------------------------------------
    if kind in BENCH_KINDS:
        for key in BENCH_ALWAYS:
            if key not in window:
                reds.append(
                    Red(
                        "WINDOW-INCOMPLETE",
                        fid,
                        f"window is missing `{key}` (kind={kind}). Both arms of "
                        "the bench window need it: the square says what was "
                        "generated and the pool says how, and CPU-seconds moves "
                        "25.4% across the pool widths this box can be asked "
                        "for. `unrecorded` is an accepted answer; silence is "
                        "not.",
                    )
                )
    if kind == "bench":
        has_gen = "radius" in window and "chunks" in window
        has_run = "level" in window and "view_distance" in window
        if not (has_gen or has_run):
            reds.append(
                Red(
                    "WINDOW-INCOMPLETE",
                    fid,
                    "a bench figure needs either radius+chunks (a generation "
                    "square) or level+view_distance (a running server); it has "
                    f"{sorted(window)}",
                )
            )
    else:
        if kind not in REQUIRED_WINDOW:
            reds.append(
                Red(
                    "MALFORMED",
                    fid,
                    f"kind `{kind}` has no declared window requirement. Add one "
                    "to REQUIRED_WINDOW rather than letting a new kind through "
                    "with no window at all.",
                )
            )
        for key in REQUIRED_WINDOW.get(kind, []):
            if key not in window:
                reds.append(
                    Red("WINDOW-INCOMPLETE", fid, f"window is missing `{key}` (kind={kind})")
                )

    for key, val in sorted(window.items()):
        if str(val).strip().lower() in PLACEHOLDERS:
            reds.append(
                Red(
                    "WINDOW-PLACEHOLDER",
                    fid,
                    f"window field `{key}` is the placeholder {val!r}. A window "
                    "field that does not apply to this kind of figure must not "
                    "be listed for it — pick or add a `kind` whose window shape "
                    "actually fits.",
                )
            )

    # -- rule 1: DOC-DRIFT ---------------------------------------------------
    #
    # Presence is not enough and finding that out cost a red demonstration that
    # came back green. A figure like `99.959781%` appears eight times in
    # `docs/PARITY.md`; editing one of the eight leaves the other seven, so a
    # substring test would pass over a doctored number. The ledger therefore
    # records **how many times** the claim is published, and any edit to any
    # occurrence moves the count. That is also why `claim_count` is mandatory:
    # an optional one would default to "at least one" and reintroduce the hole.
    doc_text = texts[fig["doc"]]
    found = doc_text.count(fig["claim"])
    if "claim_count" not in fig:
        reds.append(
            Red(
                "MALFORMED",
                fid,
                f"no `claim_count` (it is published {found} time(s) in "
                f"{fig['doc']}). Presence alone cannot detect an edit to one "
                "occurrence of a figure that appears many times.",
            )
        )
    elif found != fig["claim_count"]:
        reds.append(
            Red(
                "DOC-DRIFT",
                fid,
                f"`{fig['claim']}` appears {found} time(s) in {fig['doc']}, the "
                f"ledger says {fig['claim_count']}. Either the document changed "
                "and the ledger did not, or a published figure was edited.",
            )
        )

    # -- rule 2: ARTEFACT-DRIFT ---------------------------------------------
    art_path = ROOT / fig["artefact"]
    if not art_path.exists():
        reds.append(Red("ARTEFACT-MISSING", fid, f"{fig['artefact']} does not exist"))
    else:
        art_text = art_path.read_text(encoding="utf-8", errors="replace")
        needle = fig.get("artefact_claim")
        if needle is None:
            reds.append(Red("MALFORMED", fid, "artefact declared with no `artefact_claim`"))
        elif needle not in art_text:
            reds.append(
                Red(
                    "ARTEFACT-DRIFT",
                    fid,
                    f"`{needle}` no longer appears in {fig['artefact']} — the "
                    "artefact moved on and the published figure did not follow",
                )
            )

    # -- rule 3: ARITHMETIC --------------------------------------------------
    if "percent" in fig:
        if "residual" not in fig or "denominator" not in fig:
            reds.append(
                Red("MALFORMED", fid, "`percent` given without `residual` and `denominator`")
            )
        else:
            stated, dec = pct_to_float(fig["percent"])
            den = fig["denominator"]
            if den <= 0:
                reds.append(Red("MALFORMED", fid, f"denominator {den} is not positive"))
            else:
                computed = 100.0 * (1.0 - fig["residual"] / den)
                if f"{computed:.{dec}f}" != f"{stated:.{dec}f}":
                    reds.append(
                        Red(
                            "ARITHMETIC",
                            fid,
                            f"{fig['residual']:,} of {den:,} is "
                            f"{computed:.{dec}f}%, the ledger and the doc say "
                            f"{stated:.{dec}f}%",
                        )
                    )

    # -- rule 4: RULE 6, resolving power ------------------------------------
    #
    # `floor_comparable = false` — 2026-08-17, and it exists because rule 4's
    # PASS was as dangerous as its fail. The rule asks `floor >= residual` and
    # says nothing about whether the two numbers were produced by the same
    # instrument. On `ow-r16-admissible-n2` they were not: the residual 29,605 is
    # a charge under a rule that excludes nothing (`mask = "none"`), and the
    # floor 29,560 is the WIDTH OF THE TWIN MASK that rule does not use. So rule
    # 4 read `29,560 < 29,605`, declined to disqualify, and reported that as
    # resolving power — which is the "45 blocks of headroom" that
    # `docs/DETERMINISM.md` §14 killed on 2026-08-09, produced mechanically by
    # this checker rather than by a human. A green from a meaningless comparison
    # is this project's central defect, and it was inside the rule written to
    # prevent it.
    #
    # Measured before adding the field: 22 figures carry a `floor` under
    # `mask = "none"`, and exactly TWO of them take that floor from a different
    # instrument — `ow-r16-admissible-n2` and its restatement. The other twenty
    # carry an innocent-candidate floor from their own instrument, which is the
    # comparison rule 4 was written for. So this is a two-entry annotation, not a
    # ledger-wide migration.
    #
    # The flag does two things and both are refusals rather than allowances: rule
    # 4's arithmetic is SKIPPED (a comparison nobody should draw is not one to
    # pass), and `quotable = true` becomes red outright, because a level cannot
    # rest on a floor its own residual cannot be compared to.
    floor = fig.get("floor")
    residual = fig.get("residual")
    comparable = fig.get("floor_comparable", True)
    if comparable is False:
        if not fig.get("floor_basis"):
            reds.append(
                Red(
                    "MALFORMED",
                    fid,
                    "`floor_comparable = false` with no `floor_basis` saying what "
                    "the floor actually measures. The whole point of the flag is "
                    "to name the mismatch.",
                )
            )
        if fig["quotable"]:
            reds.append(
                Red(
                    "FLOOR-NOT-COMPARABLE",
                    fid,
                    f"quotable = true while `floor_comparable = false`. The floor "
                    f"{floor if floor is None else format(floor, ',')} and the "
                    f"residual {residual if residual is None else format(residual, ',')} "
                    "come from different instruments "
                    f"({fig.get('floor_basis')}), so rule 4's comparison has no "
                    "meaning and cannot be the thing that lets this be a level. "
                    "Take the floor with the same instrument as the residual, or "
                    "leave the figure unquotable.",
                )
            )
    if fig["quotable"] and comparable is not False:
        if floor is not None and residual is not None and floor >= residual:
            if "quotable_override" not in fig:
                reds.append(
                    Red(
                        "RULE6",
                        fid,
                        f"quotable, but the floor {floor:,} is >= the residual "
                        f"{residual:,}. A residual under the vanilla-A-vs-"
                        "vanilla-B floor has no resolving power and cannot "
                        "support an accuracy claim. Set quotable = false, or "
                        "state a `quotable_override` with its reason.",
                    )
                )
            elif not fig.get("override_reason"):
                reds.append(
                    Red("MALFORMED", fid, "`quotable_override` with no `override_reason`")
                )
    else:
        dq = fig.get("disqualifier")
        if not dq:
            reds.append(
                Red("MALFORMED", fid, "quotable = false with no `disqualifier` string")
            )
        elif not fig.get("reason"):
            reds.append(Red("MALFORMED", fid, "quotable = false with no `reason`"))
        else:
            # The reader must be able to see the disqualifier without opening
            # this ledger. A figure marked unquotable in a file nobody reads,
            # and printed bare in a file everybody reads, is the failure this
            # whole exercise is about.
            near = doc_text
            if fig["claim"] in near:
                idx = near.index(fig["claim"])
                lo = max(0, idx - 4000)
                hi = min(len(near), idx + 4000)
                if dq.lower() not in near[lo:hi].lower():
                    reds.append(
                        Red(
                            "UNQUALIFIED",
                            fid,
                            f"`{fig['claim']}` is published in {fig['doc']} with "
                            f"no `{dq}` disqualifier anywhere near it. An "
                            "unquotable figure must be qualified where it is "
                            "printed, not only in the ledger.",
                        )
                    )

            # -- rule 13: VERDICT-MARKER ---------------------------------
            #
            # PROXIMITY IS NOT ATTACHMENT, and rule 4 only has proximity.
            # It passes when the disqualifier appears anywhere in a 4,000
            # character window, which on 2026-08-13 it did for
            # `ow-r16-admissible-n2` — while 200 lines further down
            # `docs/PARITY.md` still opened a paragraph
            # "✅ **If only one *number* fits, it is 99.972345% ... and a
            # floor of 29,560 that the residual clears by 45 blocks.**"
            # A reader who finds that line has found an endorsement, and the
            # withdrawal two paragraphs above does not reach them.
            #
            # These documents already carry a verdict vocabulary and use it
            # consistently — `docs/PARITY.md` writes this very figure as
            # `99.972345% 🚫 uncontrolled chunk order` in its own table. So
            # the rule is not a new convention, it is the existing one made
            # binding: a withdrawn number may not sit on a line that says
            # the work is done.
            for line in doc_text.splitlines():
                if fig["claim"] in line and ENDORSEMENT in line:
                    reds.append(
                        Red(
                            "VERDICT-MARKER",
                            fid,
                            f"`{fig['claim']}` is not quotable ({dq}) and is "
                            f"published in {fig['doc']} on a line marked "
                            f"{ENDORSEMENT}: {line.strip()[:120]!r}. Rule 4 is "
                            "satisfied by a disqualifier anywhere within 4,000 "
                            f"characters; a reader who lands on the {ENDORSEMENT} "
                            "has found an endorsement instead. Move the marker, "
                            "not the number.",
                        )
                    )

    # -- rule 5: WINDOW-MATCH ------------------------------------------------
    if "compare_to" in fig:
        other_id = fig["compare_to"]
        other = by_id.get(other_id)
        if other is None:
            reds.append(Red("MALFORMED", fid, f"compare_to names unknown figure `{other_id}`"))
        else:
            keys = fig.get("compare_keys") or []
            if not keys:
                reds.append(Red("MALFORMED", fid, "compare_to with no `compare_keys`"))
            else:
                ow = other.get("window", {})
                for key in keys:
                    mine, theirs = window.get(key), ow.get(key)
                    if mine is None or theirs is None:
                        reds.append(
                            Red(
                                "WINDOW-MATCH",
                                fid,
                                f"compares against `{other_id}` on `{key}`, which "
                                f"one of the two does not record ({mine!r} vs {theirs!r})",
                            )
                        )
                    elif mine != theirs:
                        reds.append(
                            Red(
                                "WINDOW-MATCH",
                                fid,
                                f"compared against `{other_id}` across DIFFERENT "
                                f"windows: {key}={mine!r} against {key}={theirs!r}. "
                                "This is the r=7-against-an-r=32-target mistake.",
                            )
                        )

            # -- rules 9 and 10: the restatement pair ------------------------
            #
            # Only reached when this entry publishes its target's own number.
            # A comparison between two different quantities measured on one
            # window is the normal use of `compare_to` and neither rule
            # touches it.
            if republishes(fig, other):
                if fig["quotable"] and other.get("quotable") is False:
                    reds.append(
                        Red(
                            "RESTATED-QUOTABILITY",
                            fid,
                            f"quotable, but it republishes `{other_id}`'s own "
                            f"number and that entry is NOT quotable — "
                            f"disqualifier `{other.get('disqualifier')}`. A "
                            "restatement cannot out-rank the measurement it "
                            "restates: `quotable` on a restatement can only "
                            "mean the restatement is faithful. Withdraw it, or "
                            "restate an admissible source instead.",
                        )
                    )
                ow_keys = set(other.get("window", {}))
                missing_cmp = sorted(ow_keys - set(keys))
                if missing_cmp:
                    reds.append(
                        Red(
                            "RESTATED-WINDOW",
                            fid,
                            f"republishes `{other_id}`'s number but compares on "
                            f"only {len(keys)} of its {len(ow_keys)} window keys "
                            f"— {missing_cmp} are not compared. The same number "
                            "is the same measurement, so there is no key it may "
                            "differ on, and an omitted key is exactly how a "
                            "restatement of a disqualified figure passes the "
                            "window-match rule while disagreeing on the axis "
                            "that disqualified it.",
                        )
                    )

    # -- rule 18: PROVENANCE-NO-COMMIT --------------------------------------
    prov = str(fig.get("provenance", ""))
    # Every hex-shaped run in the string is a candidate, and the entry passes if
    # ANY of them resolves. Provenance is prose — it carries branch names, dates
    # and machine names alongside the sha — so anchoring on the first match
    # would fail on `20260812`, which is a date wearing hex's costume and is
    # eight of the ten digits' worth of proof that shape alone is not the test.
    cands = PROVENANCE_SHA.findall(prov)
    m = None
    if RESOLVE_SHAS:
        m = next((c for c in cands if not _looks_like_a_date(c) and sha_resolves(c)), None)
    elif SNAPSHOT_ERROR:
        # The transport is the finding, and it is reported once per figure
        # rather than once per tree so that a run cannot end with a single line
        # a reader skims past. See the block beside `SNAPSHOT` above for why an
        # absent stamp is a red and not a skip.
        reds.append(Red("PROVENANCE-NO-SNAPSHOT", fid, SNAPSHOT_ERROR))
        return reds
    else:
        # The answer, computed where there was history, checked against the
        # ledger line it was computed from. The digest is what stops this being
        # a verdict about a string nobody re-read: a provenance edited after the
        # stamp reads STALE rather than passing on the old resolution.
        #
        # The shape fallback that used to live here is gone. It excluded runs
        # that parse as a date — which was the right refinement of the wrong
        # rule, after `not c.isdigit()` refused the real sha `27914806` in every
        # rsynced gate directory and cost the merge chain an hour on
        # 2026-08-18 — but shape cannot separate `48d1efb`, which names no
        # object anywhere, from a sha that does. That is the case it had to
        # catch and the case it was silent on.
        row = SNAPSHOT_ROWS.get(fid)
        if row is None:
            reds.append(
                Red(
                    "PROVENANCE-SNAPSHOT-STALE",
                    fid,
                    f"no row in {SNAPSHOT.name}: this figure was added to the "
                    "ledger after the stamp was written. Re-stamp with "
                    "`ci/figure-provenance.py --snapshot` in a tree with "
                    "history and re-sync.",
                )
            )
            return reds
        digest, oid = row
        want = hashlib.sha256(prov.encode("utf-8")).hexdigest()[:16]
        if digest != want:
            reds.append(
                Red(
                    "PROVENANCE-SNAPSHOT-STALE",
                    fid,
                    f"{SNAPSHOT.name} resolved a provenance whose digest is "
                    f"{digest}; this ledger's is {want}. The `provenance` field "
                    "was edited after the stamp, so the resolution beside it is "
                    "about a different string. Re-stamp and re-sync.",
                )
            )
            return reds
        m = None if oid == "UNRESOLVED" else oid
    if not m:
        reds.append(
            Red(
                "PROVENANCE-NO-COMMIT",
                fid,
                "`provenance` names no commit. A figure whose baseline tree is "
                "unrecorded cannot be the `before` of any attribution, which is "
                "what stalled the bench-regression item. A date is not a tree — "
                "two of the twenty-five this rule was written for were recorded "
                "on the calendar day either side of the commit that holds them. "
                "`ci/figure-provenance.py` recovers it from the tree. If you "
                "believe you wrote one, it does not resolve — a sha that was "
                f"typed rather than copied reads exactly like one that was. "
                f"Candidates seen: {cands or 'none'}.",
            )
        )

    # -- rule 20: STAMP-DISAGREES -------------------------------------------
    #
    # An entry whose `artefact_claim` carries the run's own `corpus=N` stamp
    # must agree with it in `window.scenarios`. Rule 2 asserts the stamp is
    # still IN the artefact and stops there; it never compares the stamp to the
    # window, so an entry could cite a run and describe a different one.
    #
    # It did, eleven times. `docs/PARITY-ROADMAP.md` 4479 says it plainly —
    # "The denominator is 92, and every document that led here said 93" — and
    # `ci/checks/gameplay-baseline.toml` shows the arithmetic (88 written
    # against, minus `water-spreads` parked, plus five, is 92, and the recorder
    # prints `recording 92 scenario(s)`). The correction reached the documents
    # and not the ledger, so five entries said 93 against a stamp of 92 and six
    # said 93 against a stamp of 106.
    #
    # The stamp wins, always. It is what the run printed about itself; the
    # window field is what somebody typed afterwards from a brief.
    ac = str(fig.get("artefact_claim", ""))
    stamp = re.search(r"corpus=(\d+)", ac)
    if stamp:
        declared = str(fig.get("window", {}).get("scenarios", "")).strip()
        if declared and declared != stamp.group(1):
            reds.append(
                Red(
                    "STAMP-DISAGREES",
                    fid,
                    f"`window.scenarios` is {declared} and the artefact this "
                    f"entry cites stamps `corpus={stamp.group(1)}` in the very "
                    f"string rule 2 matches on. The stamp is what the run "
                    "printed about itself and the window is what someone typed "
                    "from a brief, so the stamp wins. This is the 93-that-was-"
                    "really-92 error: the correction reached the documents and "
                    "not this file.",
                )
            )

    # -- rule 11: FLOOR-UNDECLARED -------------------------------------------
    #
    # Rule 4 can only fire on an entry that carries a `floor`. Without this,
    # an exactness-shaped figure could be `quotable = true` with rule 4
    # structurally unable to reach it, and nothing in the file distinguished
    # "faced its floor and cleared it" from "never faced one".
    if fig["quotable"] and kind in FLOOR_APPLIES:
        if fig.get("floor") is None and not fig.get("floor_note"):
            reds.append(
                Red(
                    "FLOOR-UNDECLARED",
                    fid,
                    f"quotable and kind `{kind}`, with neither a `floor` for "
                    "rule 6 to fire on nor a `floor_note` saying why no floor "
                    "exists. Rule 6 cannot reach this entry, so its "
                    "quotability rests on nothing this file checks.",
                )
            )

    # -- rule 6: BASELINE ----------------------------------------------------
    baseline = fig["baseline"]
    if baseline != UNMODIFIED:
        note = fig.get("baseline_note")
        if not note:
            reds.append(
                Red(
                    "BASELINE",
                    fid,
                    f"baseline is `{baseline}`, not `{UNMODIFIED}`, and no "
                    "`baseline_note` says what it actually was. Every published "
                    "comparison must name the thing it was measured against.",
                )
            )
        elif note not in doc_text:
            reds.append(
                Red(
                    "BASELINE",
                    fid,
                    f"baseline is `{baseline}` and the note `{note}` does not "
                    f"appear in {fig['doc']}. A reader must be able to see that "
                    "this figure was not measured against unmodified vanilla.",
                )
            )

    return reds


def main(argv: list[str]) -> int:
    if not LEDGER.exists():
        print("check-figures: ci/figures.toml is missing", file=sys.stderr)
        return 2
    try:
        doc = tomllib.loads(LEDGER.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        print(f"check-figures: ci/figures.toml does not parse: {exc}", file=sys.stderr)
        return 2

    meta = doc.get("meta", {})
    figures = doc.get("figure", [])
    historical = doc.get("historical", [])
    coverage = doc.get("coverage", {})

    reds: list[Red] = []

    # Load every document once; a missing one is red before anything else.
    texts: dict[str, str] = {}
    wanted = {f.get("doc") for f in figures if f.get("doc")}
    wanted |= set(coverage.get("docs", []))
    wanted |= {h.get("doc") for h in historical if h.get("doc")}

    # `coverage.doc_globs` — 2026-08-17. The coverage scan was a hand-kept list
    # of four files, and a file that publishes an exactness percentage without
    # being on that list is scanned by nothing. Measured before widening it:
    # docs/AGENT-BRIEF.md published six percentages, docs/PARITY-ROADMAP.md one
    # and docs/PRODUCT.md one, all unscanned, and six docs/cold-start/*.md
    # published one or two each. Every one of those numbers was already
    # registered, so this widening is a ratchet that costs nothing today and
    # refuses the next unregistered one — which is the whole point, because a
    # cold start is written by an agent at the end of its run, in a hurry, and
    # is exactly where a number arrives without a window.
    #
    # A GLOB RATHER THAN MORE NAMES, for docs/cold-start/ specifically: the
    # directory gains a file per branch, so a hand-kept list would go stale by
    # construction on the next agent and nothing would say so. The named
    # entries in `coverage.docs` stay named, because those are the claim
    # documents and their membership is a decision rather than a pattern.
    coverage_docs = list(coverage.get("docs", []))
    for pattern in coverage.get("doc_globs", []):
        matched = sorted(str(p.relative_to(ROOT)) for p in ROOT.glob(pattern))
        if not matched:
            # A glob that matches nothing is a scan that cannot fail. It is the
            # `min_external_artefacts` fault in another costume, so it is red
            # rather than an empty loop.
            reds.append(
                Red(
                    "COVERAGE-GLOB-EMPTY",
                    "-",
                    f"`coverage.doc_globs` entry {pattern!r} matches no file. A "
                    "scan over an empty set is a check that cannot go red; "
                    "delete the pattern or fix it.",
                )
            )
        coverage_docs.extend(matched)
    coverage_docs = sorted(set(coverage_docs))
    wanted |= set(coverage_docs)
    for rel in sorted(wanted):
        path = ROOT / rel
        if not path.exists():
            reds.append(Red("DOC-MISSING", "-", f"{rel} does not exist"))
            texts[rel] = ""
        else:
            texts[rel] = path.read_text(encoding="utf-8", errors="replace")

    by_id = {f.get("id"): f for f in figures}
    if len(by_id) != len(figures):
        reds.append(Red("MALFORMED", "-", "two ledger entries share an `id`"))

    for fig in figures:
        reds.extend(check_one(fig, by_id, texts))

    # -- self-assertion: the ledger has not been emptied ---------------------
    min_figures = meta.get("min_figures", 0)
    if len(figures) < min_figures:
        reds.append(
            Red(
                "LEDGER-SHRANK",
                "-",
                f"{len(figures)} figures registered, floor is {min_figures}. "
                "Deleting entries is how a ledger-driven check becomes one that "
                "cannot fail.",
            )
        )

    # -- self-assertion: every rule is exercised by something ----------------
    n_unquotable = sum(1 for f in figures if f.get("quotable") is False)
    if n_unquotable < meta.get("min_unquotable", 0):
        reds.append(
            Red(
                "RULE-UNEXERCISED",
                "-",
                f"only {n_unquotable} entries exercise the rule-6 path, floor is "
                f"{meta.get('min_unquotable')}. A rule no entry exercises is not "
                "a rule.",
            )
        )
    n_compare = sum(1 for f in figures if "compare_to" in f)
    if n_compare < meta.get("min_compare_pairs", 0):
        reds.append(
            Red(
                "RULE-UNEXERCISED",
                "-",
                f"only {n_compare} entries exercise the window-match path, floor "
                f"is {meta.get('min_compare_pairs')}. The r=7-vs-r=32 guard must "
                "have something to guard.",
            )
        )

    # Rules 9 and 10 reach only entries that republish their target's number,
    # and after the 2026-08-13 fix no entry violates either — which is exactly
    # the shape this file's header warns about. Two things stop them becoming
    # decoration: this floor, so the restatement relation cannot quietly leave
    # the ledger, and `ci/check-figures-falsify.py`, which injects each fault
    # and asserts the rule fires. A rule proven only by the absence of a
    # violation is not proven.
    n_restate = sum(
        1
        for f in figures
        if "compare_to" in f
        and by_id.get(f["compare_to"]) is not None
        and republishes(f, by_id[f["compare_to"]])
    )
    # -- rules 4 and 13 ONLY EVER READ THE FIGURE'S OWN `doc` ----------------
    #
    # 2026-08-17. Both rules take `doc_text = texts[fig["doc"]]`, so a withdrawn
    # figure republished in ANY OTHER tracked document is invisible to them. That
    # is not hypothetical: `docs/WORKLIST.md` line 7890 called the withdrawn
    # 29,605 "the quotable figure" for a week, and `docs/AGENT-BRIEF.md` — the
    # file every agent reads first and none re-checks — carries four withdrawn
    # claims. The ledger's own `ow-r16-admissible-n2` provenance even points at
    # `docs/WORKLIST.md` as a place the number is published, while no rule looked
    # there.
    #
    # Measured across all 72 scanned documents before choosing the shape: 65
    # mentions of a withdrawn claim outside its own doc, 0 of them on an
    # endorsement line, and 24 individual occurrences with no disqualifier within
    # 4,000 characters of that occurrence.
    # Those two counts want two different rules and it would be wrong to make
    # them one:
    #
    # ECHO-VERDICT-MARKER is ABSOLUTE. Zero violations today and it must stay
    # zero — printing a withdrawn number on a line that says the work is settled
    # is never right, wherever it happens. A rule with no current violation is
    # this checker's own warned-about shape, so it is held by
    # `ci/check-figures-falsify.py` injecting one rather than by the absence.
    #
    # ECHO-UNQUALIFIED is a RATCHET. 24 occurrences today, and they are not all
    # defects: a percentage string recurring in a table, or a document that
    # states the withdrawal in its own words rather than in the ledger's
    # `disqualifier` wording, both land here. Auditing 24 of them is a reading
    # job across four claim documents and is not this branch's; what is worth
    # having immediately is that the count cannot RISE, because every one of
    # those 35 got there by somebody adding a mention and nothing objecting.
    echo_reds, n_echo_unqual = check_echoes(figures, texts)
    print(
        f"FIGURES echoes: {n_echo_unqual} unqualified mentions of a withdrawn "
        f"claim outside its own doc (ceiling "
        f"{meta.get('max_unqualified_echoes')}), "
        f"{len(echo_reds)} on an endorsement line (ceiling 0)"
    )
    reds.extend(echo_reds)
    max_echo = meta.get("max_unqualified_echoes")
    if max_echo is None:
        reds.append(
            Red(
                "MALFORMED",
                "-",
                "`[meta] max_unqualified_echoes` is absent; the ECHO-UNQUALIFIED "
                "ratchet has no ceiling and cannot fail.",
            )
        )
    elif n_echo_unqual > max_echo:
        reds.append(
            Red(
                "ECHO-UNQUALIFIED",
                "-",
                f"{n_echo_unqual} mentions of a withdrawn figure's claim sit in a "
                f"document other than its own with no disqualifier within "
                f"{NEAR} characters, ceiling is {max_echo}. A reader who lands on "
                "one has found a bare number. Print the disqualifier beside it, "
                "or cite the figure by id instead of restating its value.",
            )
        )

    # -- `min_external_artefacts`: A DECLARED FLOOR NO CODE HAD EVER READ ----
    #
    # 2026-08-17. `[meta] min_external_artefacts = 0` has sat in
    # `ci/figures.toml` since the ledger was written and `git log -S` finds no
    # commit in this script's whole history that reads it. It is this project's
    # central defect wearing a number: a floor that decides nothing, in the file
    # whose job is to stop numbers deciding nothing. Note that reading it would
    # not have been enough either — `>= 0` is satisfied by an empty ledger — so
    # the fix is both halves, the read and a floor that can fail.
    #
    # What it means, and it is the reason the key is worth keeping rather than
    # deleting: an `artefact` that is another tracked document is a doc citing a
    # doc, and rule 2 then compares prose to prose. An artefact OUTSIDE the
    # documents — a script, a floor table, a census dump — is the half that
    # catches a *measurement* that moved rather than a *sentence* that was
    # edited. 73 of 93 figures have one. The floor guards the ratio, not the
    # count: it is set well under 73 so a handful can be re-taken, and refuses
    # the ledger quietly becoming self-referential.
    external = [
        f
        for f in figures
        if f.get("artefact") and not str(f["artefact"]).endswith(".md")
    ]
    min_external = meta.get("min_external_artefacts")
    if min_external is None:
        reds.append(
            Red(
                "MALFORMED",
                "-",
                "`[meta] min_external_artefacts` is absent. It is read now; a "
                "missing floor would silently restore the state this rule was "
                "written to end.",
            )
        )
    elif len(external) < min_external:
        reds.append(
            Red(
                "EXTERNAL-ARTEFACTS",
                "-",
                f"only {len(external)} of {len(figures)} figures cite an "
                f"artefact outside the documents, floor is {min_external}. An "
                "artefact that is itself a tracked document makes rule 2 "
                "compare prose to prose; the external half is what catches a "
                "measurement that moved rather than a sentence that was edited.",
            )
        )

    # -- SELF-ARTEFACT: rule 2 may not re-check rule 1's file ----------------
    #
    # 2026-08-17, found while implementing the floor above and sharper than it.
    # Seven entries set `artefact` to the SAME file as `doc`. For those seven
    # rule 2 (ARTEFACT-DRIFT) reads the document rule 1 (DOC-DRIFT) already
    # read, so it cannot fail unless rule 1 has already failed: the measurement
    # half of this checker's flagship guard is structurally vacuous on them,
    # exactly as this script's own header says it must not be. `artefact` being
    # a DIFFERENT document is fine and there are thirteen of those — a headline
    # in one claim doc backed by the table in another is a real cross-check.
    #
    # A RATCHET, NOT AN ABSOLUTE, for the reason `max_quotable_without_dump_order`
    # is one: closing an entry means re-deriving where its number actually came
    # from, which is a measurement and not a bookkeeping edit. Two of the seven
    # sit in `docs/BENCH.md`, which a performance agent owns and which has no
    # merge driver. So the count may not RISE, and each comes off as it is
    # re-taken. Raising it to admit a new entry is the one move that is never
    # right.
    self_art = sorted(
        f.get("id", "?") for f in figures if f.get("artefact") and f["artefact"] == f.get("doc")
    )
    max_self = meta.get("max_self_artefact")
    if max_self is None:
        reds.append(
            Red(
                "MALFORMED",
                "-",
                "`[meta] max_self_artefact` is absent; the SELF-ARTEFACT ratchet "
                "has no ceiling and cannot fail.",
            )
        )
    elif len(self_art) > max_self:
        reds.append(
            Red(
                "SELF-ARTEFACT",
                "-",
                f"{len(self_art)} entries have `artefact` equal to `doc`, "
                f"ceiling is {max_self}: {', '.join(self_art)}. Rule 2 then "
                "re-reads the file rule 1 already read and cannot fail "
                "independently of it. Point `artefact` at the script, table or "
                "census the number came from, or at another document that "
                "carries it.",
            )
        )

    if n_restate < meta.get("min_restatements", 0):
        reds.append(
            Red(
                "RULE-UNEXERCISED",
                "-",
                f"only {n_restate} entries republish another entry's number, "
                f"floor is {meta.get('min_restatements')}. Rules 9 and 10 reach "
                "nothing else, so with none of these they are unreachable.",
            )
        )

    # -- rule 12: DUMP-ORDER-UNRECORDED, as a ratchet ------------------------
    #
    # `docs/DETERMINISM.md` §15 prices a wrong chunk order at 117,628 blocks on
    # a candidate that IS unmodified vanilla, against 3,804 for being a
    # different run of the right one. A quotable worldgen figure that does not
    # record which order its own dump walked is therefore not a weaker claim
    # than one that does — it is an unconditioned one.
    #
    # It is a ratchet rather than an absolute because filling in the twelve
    # that are missing today is a MEASUREMENT and not a bookkeeping edit: the
    # honest value for several is recoverable only by re-diffing against a
    # stamped reference (AGENT-BRIEF, ORACLE rule 30 — "an assertion is not a
    # stamp"), and writing a guess into a window is precisely the defect this
    # ledger exists to prevent. So the set is made explicit and frozen: a new
    # quotable worldgen figure has to record its dump order, and each of the
    # twelve can be retired as it is re-taken by lowering this number.
    unrecorded = sorted(
        f["id"]
        for f in figures
        if f.get("quotable") is True
        and f.get("kind") in CHUNK_ORDER_KINDS
        and "dump_order" not in f.get("window", {})
    )
    ceiling = meta.get("max_quotable_without_dump_order", 0)
    if len(unrecorded) > ceiling:
        reds.append(
            Red(
                "DUMP-ORDER-UNRECORDED",
                "-",
                f"{len(unrecorded)} quotable chunk-order-sensitive figures record "
                f"no `dump_order`, ceiling is {ceiling}: {unrecorded}. Record the "
                "order this figure's dump walked, or lower nothing and take the "
                "measurement — do not raise the ceiling to admit a new one.",
            )
        )

    # -- rule 17: BENCH-WORKERS-UNRECORDED, a ratchet --------------------------
    #
    # Same shape as rule 12 and for the same reason. Requiring the field only
    # makes the gap sayable; it does not close it, and the entries that predate
    # `ci/bench-servers.sh` stamping the pool cannot be repaired by editing this
    # file — they have to be re-taken. So the count is frozen and may only fall.
    #
    # Corroboration, which is not the same as a record: the 2026-08-14
    # head-to-head at `~/bench-h2h/results.tsv` shows our peak-core column at
    # 11.6, 12.1 and 12.1 across three squares on this box, consistent with the
    # 12-core default and inconsistent with an SMT-wide pool. That makes the
    # published ratios likely right; it does not make them recorded, and this
    # ledger's whole premise is that the difference matters.
    bench_unrecorded = sorted(
        f["id"]
        for f in figures
        if f.get("quotable") is True
        and f.get("kind") in BENCH_KINDS
        and str(f.get("window", {}).get("workers", "")).strip().lower() == "unrecorded"
    )
    bench_ceiling = meta.get("max_quotable_bench_without_workers", 0)
    if len(bench_unrecorded) > bench_ceiling:
        reds.append(
            Red(
                "BENCH-WORKERS-UNRECORDED",
                "-",
                f"{len(bench_unrecorded)} quotable bench figures record no worker "
                f"pool width, ceiling is {bench_ceiling}: {bench_unrecorded}. "
                "Re-take the measurement with `ci/bench-servers.sh`, which now "
                "pins and stamps it — do not raise the ceiling to admit a new "
                "one.",
            )
        )

    # -- rule 19: DASHBOARD-UNREGISTERED, a ratchet -------------------------
    #
    # THE COVERAGE SCAN MATCHES `[0-9]+\.[0-9]{4,}%` AND THE DASHBOARD IS MADE
    # OF COUNTS. `docs/PARITY.md` 2 is the one table a reader actually lands on
    # and every cell in it — blocks, residual, floor — is a bare number, so the
    # UNREGISTERED scan that guards every percentage in seven documents could
    # not see a single figure in it. Found 2026-08-18 while giving that table
    # its floor column: five of its twelve residuals are in no ledger entry.
    #
    # Two of the five are the sharpest, because their siblings ARE registered:
    # 8,472 and 39,941 are the RAW residuals of rows whose order-invariant
    # cores (8,459 and 6,127) each have a full entry with a window and a floor.
    # The raw half is published beside the core, in the same row, with none of
    # that discipline — and 39,941 is 2.4x under its own floor, which is only
    # visible at all because the floor column now exists.
    #
    # A RATCHET AND NOT A FLOOR, for the usual reason: registering the five
    # means measuring five windows, and inventing a window to satisfy the
    # ledger is the defect the ledger exists to prevent. What is enforced is
    # that a SIXTH cannot arrive. A row whose figure cell says `not measured`
    # is not counted — that is an honest statement of absence, and it is the
    # distinction the old table had no column to make.
    # Read from disk rather than from `texts`, which only holds documents some
    # figure happens to cite. This rule is about one named file whether or not
    # anything points at it — the dashboard going unregistered is precisely the
    # case where nothing does.
    dash_path = ROOT / "docs" / "PARITY.md"
    dash = dash_path.read_text(encoding="utf-8") if dash_path.exists() else ""
    dash_unreg: list[str] = []
    if "## 2. Where parity stands" in dash:
        body = dash[dash.index("## 2. Where parity stands"):]
        nxt = body.find("\n## ", 10)
        body = body[:nxt] if nxt > 0 else body
        known = {
            v
            for f in figures
            for k in ("residual", "denominator", "floor")
            if isinstance(v := f.get(k), int)
        }
        for line in body.split("\n"):
            if not line.startswith("| ") or "---" in line:
                continue
            cells = [c.strip() for c in line.strip("|").split("|")]
            if len(cells) < 5 or cells[4].lower().startswith(("residual", "not measured")):
                continue
            digits = re.sub(r"[^\d]", "", cells[4])
            if not digits:
                continue
            if int(digits) not in known:
                dash_unreg.append(f"{cells[0]}/{cells[1]} {cells[4]}")
    dash_ceiling = meta.get("max_unregistered_dashboard_rows")
    if dash_ceiling is None:
        reds.append(
            Red(
                "DASHBOARD-UNREGISTERED",
                "-",
                "`[meta] max_unregistered_dashboard_rows` is absent. It is the "
                "ratchet on the one table in this project made entirely of "
                "counts, which the percentage-shaped coverage scan cannot see.",
            )
        )
    elif len(dash_unreg) > dash_ceiling:
        reds.append(
            Red(
                "DASHBOARD-UNREGISTERED",
                "-",
                f"{len(dash_unreg)} residuals in docs/PARITY.md 2 are in no "
                f"ledger entry, ceiling is {dash_ceiling}: {dash_unreg}. The "
                "dashboard is the table a reader lands on and it is made of "
                "counts, so the percentage-shaped coverage scan never reached "
                "it. Register the figure with its window, or write `not "
                "measured` in the row, which is counted as the honest answer it "
                "is — do not raise the ceiling.",
            )
        )

    # -- coverage: every declared gate floor must be registered -------------
    #
    # The generalisation of a real miss. `min_binaries` went 97 -> 106 on
    # `main` while this ledger sat unmerged, and two brand-new floors —
    # `min_tests` and `max_ignored` — appeared alongside it. The first was
    # caught because it happened to be registered; the two new ones would have
    # gone unregistered indefinitely, which is the "floor that rots silently"
    # shape. So the rule is not "these three floors" but "every floor".
    #
    # A floor is merge-blocking by construction: it is the number that decides
    # whether a branch lands. That makes it a published figure under this
    # ledger's definition, and it needs an artefact — the adjudicator that
    # reads it — for the same reason every other figure does. A floor sitting
    # in a toml after the code enforcing it was deleted is a check that cannot
    # fail, wearing a number.
    floor_globs = coverage.get("floor_files", [])
    if floor_globs:
        floor_rx = re.compile(r"^((?:min|max)_[a-z0-9_]+)\s*=\s*([0-9]+)\s*$", re.M)
        registered_floors = {
            f["claim"] for f in figures if f.get("kind") == "gate-floor"
        }
        for glob in floor_globs:
            for path in sorted(ROOT.glob(glob)):
                rel = path.relative_to(ROOT).as_posix()
                body = path.read_text(encoding="utf-8", errors="replace")
                for name, value in floor_rx.findall(body):
                    decl = f"{name} = {value}"
                    if decl not in registered_floors:
                        reds.append(
                            Red(
                                "UNREGISTERED-FLOOR",
                                rel,
                                f"`{decl}` is a merge-blocking gate floor and is "
                                "not registered as a `gate-floor` figure. It "
                                "needs the run that set it and the adjudicator "
                                "that reads it, or nothing notices when either "
                                "goes away.",
                            )
                        )

    # -- coverage: no unregistered figure in a claim document ---------------
    pattern = coverage.get("pattern")
    if pattern:
        registered = {f["percent"] for f in figures if "percent" in f}
        retired = {h["value"] for h in historical if "value" in h}
        rx = re.compile(pattern)
        for rel in coverage_docs:
            for token in sorted(set(rx.findall(texts.get(rel, "")))):
                if token not in registered and token not in retired:
                    reds.append(
                        Red(
                            "UNREGISTERED",
                            rel,
                            f"`{token}` is published in {rel} and is in neither "
                            "the figure ledger nor the retired list. Every "
                            "number a reader could act on needs an artefact and "
                            "a window.",
                        )
                    )

    # -- report --------------------------------------------------------------
    print(
        f"FIGURES: {len(figures)} registered, {n_unquotable} unquotable, "
        f"{n_compare} cross-window comparisons, {n_restate} restatements, "
        f"{len(historical)} retired"
    )
    # The census is printed on every run, green included, because the whole
    # point of rules 9-12 is that "quotable" stops being an unexamined word.
    # A reader of the gate log can see how each quotable entry earned it.
    adjudicated: dict[str, int] = {}
    for f in figures:
        if f.get("quotable") is not True:
            continue
        if f.get("kind") not in FLOOR_APPLIES:
            basis = "kind-exempt"
        elif f.get("floor") is not None and f.get("residual") is not None:
            basis = "override" if "quotable_override" in f else "rule-6"
        else:
            basis = "floor-note"
        adjudicated[basis] = adjudicated.get(basis, 0) + 1
    print(
        "FIGURES quotable basis: "
        + ", ".join(f"{k} {v}" for k, v in sorted(adjudicated.items()))
        + f"; {len(unrecorded)} without dump_order (ceiling {ceiling})"
        + f"; {len(bench_unrecorded)} bench without workers "
          f"(ceiling {bench_ceiling})"
        + f"; {len(dash_unreg)} dashboard residuals unregistered "
          f"(ceiling {dash_ceiling})"
    )
    if reds:
        print()
        for r in reds:
            print(f"FIGURES RED [{r.rule}] {r.fid}: {r.detail}")
        print()
        print(f"FIGURES RED — {len(reds)} failing rule(s)")
        return 1

    print("FIGURES GREEN")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
