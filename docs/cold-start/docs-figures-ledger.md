# COLD START — branch `docs-figures-ledger`

### COLD START — branch `docs-figures-ledger`, 2026-08-17

Area: the docs-and-figures ledger — the headline dashboard, and every figure
that outlived its measurement. Off `main` `bba29ee2`.

## `docs/PARITY.md` WAS BLOCKED AND IS STILL BLOCKED — nothing in it was touched

    git merge-base --is-ancestor origin/clamped-floor-probe origin/main  -> 1

`origin/clamped-floor-probe` is `11ff71d8` and is **not** an ancestor of
`origin/main`. That branch rewrites `docs/PARITY.md`, so this branch did not
open it. **Everything the brief listed inside that file is still outstanding**
and belongs to whoever lands `clamped-floor-probe` or the next ledger agent
after it:

* the §2 dashboard around line 339 (`69,288 ✅`) still has **no floor column at
  all**, three refinements and a full ladder generation stale;
* line ~350 "the 20260722 r=16 window has NO quotable exactness figure, in this
  table";
* line ~359 "the three r=16 overworld rows are STALE";
* line ~247 the 2026-08-06 N=6 route;
* line **1331** still carries the un-struck `clears by 45 blocks`
  mask-subtraction. `docs/PARITY.md:204` was struck long ago; 1331 was not.
  This branch struck the same error in the two files that are **not** blocked.
* the clamped / unclamped / both publish choice is three unpicked options on
  `clamped-floor-probe` and is **the owner's**. Not picked, not nudged, not
  tidied into a recommendation.

**The current measurements, so nobody republishes a dead number:** the r=16
clamped floor is **NOT quotable** — the clamp removes 45.1% of the harvest floor
(median 16,302.5 -> 8,951) and 15 of 15 pairs stay non-zero. The admissible
ladder at N=5 reads unclamped charge 1,330..1,884 against floor 2,577..4,835,
clamped 1,917..2,333 against floor 2,006..3,265 — **below floor in both arms**.
The block-entity channel scores `173 vs 71, id=0 payload=0 exact=68`.

## What was stale, and where it actually was

The brief named `docs/PARITY.md:1334`. The same error was in **two more files**,
one of them found only by sweeping:

| file | state |
| --- | --- |
| `docs/PARITY.md:204` | already struck, before this branch |
| `docs/PARITY.md:1331` | **un-struck, BLOCKED**, see above |
| `docs/WORKLIST.md:7890` | **struck by this branch** |
| `docs/PREREG-ruined-portal-blocks.md:275` | **corrected by this branch**, by append |
| `docs/WIDE-WINDOW-R16-ALL-DIMENSIONS.md:220` | **NOT a defect** — see the nulls |
| `docs/cold-start/ci-figures-ledger.md:104` | quotes it *as* the known error; correct as-is |

`docs/WORKLIST.md:7890` said "**The quotable figure clears the floor by 45
blocks.** 29,605 against 29,560" — two errors in one sentence. The subtraction
takes the width of a **twin mask** off a charge that uses **no mask**
(`docs/DETERMINISM.md` §14, 2026-08-09: an innocent unmodified vanilla run is
itself charged **16,932** here at N=2). And 29,605 has not been quotable since
2026-08-10.

`docs/WORKLIST.md:70` claimed "`docs/BENCH.md` §3 calls the drift unattributed
and never considers it". §3 has been headed ATTRIBUTED since 2026-08-17 and
carries the trial-chambers attribution (+7.03% instructions/chunk, n=6, CI
3.02-11.19%). Struck, **with the ~+14% remainder explicitly still open** so the
correction cannot be read as closing it.

## Checks made falsifiable, and how each was proven

Every one was proven by mutating an input, confirming the red names its mode,
restoring, and confirming green. `ci/ledger-tree-checks.sh` is new — the
tree-only subset of `ci/t0.sh`, six checks, so a docs or checks commit can be
gated without occupying the Threadripper.

**1. The coverage scan's own membership.** `[coverage] docs` was four hand-kept
filenames, so a document publishing an exactness percentage while not on that
list was scanned by **nothing**. Twelve such documents, measured before
widening: `docs/AGENT-BRIEF.md` (6 percentages), `docs/PARITY-ROADMAP.md` (1),
`docs/PRODUCT.md` (1), six `docs/cold-start/*.md` (1-2 each) — **every number
already registered**, so the widening cost no re-measurement. `coverage.doc_globs`
is new and carries `docs/cold-start/*.md`, which gains a file per branch; an
empty glob is itself red. Proof: injections into `AGENT-BRIEF.md` and a
cold-start file are **GREEN on main's checker, exit 0**, RED on this one.

**2. `min_external_artefacts` — declared since the ledger was written and never
read.** `git log -S` finds no commit in `ci/check-figures.py`'s history reading
it, and it read `0`, which an empty ledger satisfies; both halves fixed. Proof:
floor of **999999 is GREEN on main's checker**.

**3. SELF-ARTEFACT, sharper than the floor that led to it.** Seven entries set
`artefact` equal to `doc`, so rule 2 (ARTEFACT-DRIFT) re-reads the file rule 1
already read and **cannot fail independently of it** — the measurement half of
this checker's flagship guard, structurally vacuous. Ratcheted at 7, may not
rise; two of the seven live in `docs/BENCH.md`, which is not this branch's file.

**4. DANGLING-SECTION-REF in `ci/check-doc-claims.py`.** All four existing rules
run between `docs/claims/*.toml` and the headings; none looked at the **310
places that cite a section by number**. The sharp proof is a renumber done
*correctly* — heading, claim body and claim filename all moved together, which
is what a careful agent does: main reports "79 claims registered, 0 collisions,
0 drift" and **exits 0** while 19 citations point at nothing. Ratcheted at 8
because all eight of today's danglers are forward references to unmerged
sections; the count is printed on every green run so the ceiling can be lowered
as they land.

**5. Two isolation holes in `ci/check-figures-falsify.py`** (13 cases -> 18):
`meta` is now `deepcopy`d, and a case supplying a document outside `DOCS` is
refused, so "`run()` rewrites every doc each call" is true by construction
rather than by luck.

## NULLS — do not re-run these

* **A prose detector for the mask-subtraction construction was prototyped and
  DELIBERATELY NOT SHIPPED.** Regex over paragraph scope, keyed on the residual
  or percent of any `quotable = false` figure plus a positive floor-clearance
  phrase. Measured across `README.md` and all of `docs/`: **8 hits, 1 real
  defect, 6 false positives, 1 blocked.** The false positives are paragraphs
  that exist *to debunk* the construction (`docs/DETERMINISM.md` §14's own
  heading region, the "framing that died" section) plus legitimate
  same-instrument floor comparisons. A ~75% false-positive T0 red is a check
  agents route around, which is worse than none. The one real defect was fixed
  by hand instead. **Do not rebuild this as a regex.** If it is worth
  mechanising, the sound version keys on the ledger, not on prose.
* **`docs/WIDE-WINDOW-R16-ALL-DIMENSIONS.md:220` is not a defect.** It reads
  "clears the floor by only 45 blocks" un-struck, but the **immediately
  following** block is `> ⚠ **Two corrections to the sentence above**`. That is
  attachment, not proximity. Left alone.
* **`docs/BENCH.md:382` "nobody has attributed that increment" is CORRECT.** It
  refers to the further **~+14%** from `9b4afd4e` to `35b3bde5`, which is open
  and being worked separately — not to the stale tenth, which is attributed.
  Do not "fix" it.
* **The cold-start index is clean.** 64 rows audited mechanically: **0 dead
  links, 0 cold-start files without a row, 0 dangling section citations.** Every
  `cold-start/*.md` link in the tree resolves. 56 of 64 rows name a merged
  branch and 7 name a deleted one — that is a historical record working as
  intended, not rot. Nothing to sweep.
* **`ci/check-doc-claims.py`'s pre-existing four rules are SOUND.** Re-confirmed
  by falsification, not inherited: DUPLICATE-CLAIM, DUPLICATE-HEADING,
  HEADING-NO-CLAIM, CLAIM-NO-HEADING and FILENAME-MISMATCH each fire. The gap
  was citations, which is now rule 5. **Do not re-audit these five.**
* **`docs/PREREG-ruined-portal-blocks.md`'s two percentages must NOT be
  registered.** They are *predictions* made before measurement. Registering one
  hands it an artefact and a window it never had, and a consumer enumerating the
  ledger reads it as a result. Recorded in `ci/figures.toml` beside the scan
  list.

## `docs/BENCH.md` IS NOT A MERGE-REGION CANDIDATE — asked and answered

Assessed on request, and the answer is the same as `ci/t1.sh`'s: **no.** It is a
modify-in-place measurement file, not an append-only one.

* **21 of its last 40 commits DELETE lines** (+2,183 / -188 overall). The commit
  subjects say what they are: "corrected in place", "retracted as a sampling
  artefact", "say plainly that the 40 MB after-row is at radius 2".
* **Edits are scattered through existing sections**, not gathered at an anchor:
  hunk starts across the last six commits are lines 17, 79, 112, 132, 234, 261,
  318, 333, 344, 581 and 1313.
* `ci/merge-docregions.py` resolves only inside a declared region and only via a
  finer 3-way answer; it **cannot keep both sides of a rewritten line**, and
  keeping both would publish two contradictory figures — the union damage that
  put `ci-unfalsifiable-root` in `main` twice, at `10 of 84` and `4 of 84`.

New sections *are* prepended near line 17, so a **narrow top-of-file region**
would help the "two agents each add a new section" case — but that was 2 of 6
sampled commits, and the marker lines would sit in the single most contested
zone of the file while a performance agent is live in it. Not done, and not
recommended until that agent is finished.

## Two more rules, and the one that paid for itself immediately

**Rules 4 and 13 only ever read a figure's own registered `doc`.** Both take
`texts[fig["doc"]]`, so a withdrawn figure republished in any other tracked
document was invisible to both — which is exactly how `docs/WORKLIST.md:7890`
stood for a week. Measured across all 72 scanned documents: **65 mentions of a
withdrawn claim outside its own doc, 0 on an endorsement line, 24 occurrences
with no disqualifier within 4,000 characters.** Two counts, so two rules:
`ECHO-VERDICT-MARKER` absolute at 0, `ECHO-UNQUALIFIED` a ratchet at 24.

> **A correction to my own first measurement, recorded because it is this file's
> own subject matter.** The first pass reported **35** unqualified echoes by
> asking the question once per document, against each document's *first*
> occurrence only. Per occurrence — which is what the checker computes — it is
> **24**. The looser count was the more alarming one, and publishing it would
> have been quoting an instrument nobody had read.

**Then adding `docs/ORACLE.md` to the coverage scan took the echo count 24 -> 25
and named the new one.** `docs/ORACLE.md:1567` published *"the corpus floor is 9
of 88 scenarios"* as current. That is `corpus-floor-n6`,
`quotable = false, disqualifier = "destroyed"` — its six recordings all lived at
one path in one tree, each `ci/t2-gameplay.sh` run overwriting the last, and no
surviving file's sha256 matches any of them. **The live floor is 7 of 88**
(`corpus-floor-n6-gpfloors`, `quotable = true`, bit-identical shas recorded).
`docs/ORACLE.md` now says so and says which to quote. **The ceiling was not
raised to admit it** — the violation was fixed and the count is back to 24
against an unchanged 24. Raising a ratchet to accept a new violation is the one
move that is never right.

The four percentages in `docs/ORACLE.md`, `docs/CI.md` and `docs/PHASE0.md` are
**closed**, and none of them wanted a `[[figure]]` entry: two are inherited r18
seed-20260708 ratchet targets from another project with no artefact in this repo,
one is a positive control whose value is beside the point (it proves two
invocations *agree*), and one is a diagnostic before/after whose prose records no
window. All four are retired, each with the reason it is not a level. Three
documents joined the scan; the ledger is 93 figures and 10 retired.

> **Why this file cites numbers by line rather than by digits.**
> `docs/cold-start/*.md` is inside the coverage scan now, so writing four
> unregistered percentages out here published them into a scanned document and
> reddened `ci/check-figures.py` four times — correctly. A cold start naming a
> number *as unregistered* is not the same act as a document publishing it, but
> the scan cannot tell them apart and should not try. Pointing at where a number
> lives is also what stops this file becoming one more place each figure can
> drift.

## The sharpest single finding of the run: rule 4's PASS was the 45-blocks error

Left for last because it is the one worth reading twice. **This checker was
generating the error the rest of this branch spent the day striking out of the
prose.**

Rule 4 asks `floor >= residual` and never asked whether the two numbers came from
the same instrument. On `ow-r16-admissible-n2` they do not:

    residual 29,605   a charge under a rule that EXCLUDES NOTHING (mask = none)
    floor    29,560   the WIDTH OF THE TWIN MASK that rule does not use

So rule 4 read `29,560 < 29,605`, declined to disqualify, and reported the
difference as resolving power. **That difference is "45 blocks of headroom".**
Flip that entry to `quotable = true` on the pre-2026-08-17 checker and it is
**GREEN, exit 0**. Every hand-written instance of this error that
`docs/DETERMINISM.md` §14 killed on 2026-08-09 had a mechanical twin sitting in
the gate, unexamined, for eight days.

`FLOOR-NOT-COMPARABLE` closes it. `floor_comparable = false` + `floor_basis` make
rule 4 **skip** the arithmetic (a comparison nobody should draw is not one to
pass) and make `quotable = true` red outright. Measured first, so it is a
two-entry annotation rather than a ledger-wide migration: **22 figures carry a
floor under `mask = "none"`, exactly two take it from a different instrument** —
this entry and its restatement. The other twenty carry an innocent-candidate
floor from their own instrument, which is what rule 4 was written for. The floors
are annotated, not deleted: 29,560 is the number the prose quoted and struck, and
`floor_basis` records the comparable one — **16,932 at N=2, which 29,605 does NOT
clear.**

## ECHO-UNQUALIFIED worked 24 -> 14, and one of the ten was a wrong headline

Nine were **vocabulary**: banners reading "IS HISTORY", "superseded", "went past
it", "no longer exist" — qualifying their numbers perfectly well in substance
while using none of the ledger's `disqualifier` words, so the rule could not see
the qualification was there. Fixed by naming the ledger's own word beside the
number. No verdict changed.

**One was a wrong number, in the worst possible file.** `docs/AGENT-BRIEF.md`
called `69,288 / 99.935277%` *the project's one quotable worldgen exactness*,
while the ledger has that reading as
`quotable = false, disqualifier = "superseded"`. **The live figure is 99.992583%
— 7,940 of the same denominator** — after the `IcebergFeature` port dropped the
charge 61,348 blocks (88.5%) **with the floor unmoved** at 2,577..4,835, a floor
being a property of the reference ensemble that the port did not touch.
`docs/AGENT-BRIEF.md` records that very port 400 lines further down, so the two
halves of one document disagreed for a week — and it is the half every agent
reads first and none re-checks. Corrected.

**The remaining 14 are all in `docs/DETERMINISM.md` and are deliberately left.**
Ten of them sit in its summary preamble, the highest-collision region of the
highest-collision file in the repo (five of six blocked worldgen branches on the
morning of 2026-08-17 were conflicts in it). They are vocabulary, not wrong
numbers. Whoever is already editing that file should take them **and lower the
ceiling again** — the reason is recorded beside the ceiling in `ci/figures.toml`.

## THE SINGLE NEXT ITEM

**`docs/PARITY.md`, and it is the sharpest thing left in this area** — but only
once this returns 0:

    git merge-base --is-ancestor origin/clamped-floor-probe origin/main

Everything listed at the top of this file is still outstanding in it: the §2
dashboard around line 339 with **no floor column at all**, three refinements and
a full ladder generation stale; the stale r=16 rows; the N=6 route at line ~247;
and the un-struck `clears by 45 blocks` at line **1331**. **What closes it:**
every figure in the §2 table carrying its floor and its N beside it, with no
floor subtracted from any residual, and `ci/check-figures.py` green.
**What must not happen:** picking the clamped / unclamped / both publish option.
Those are three unpicked options on `clamped-floor-probe` and the choice is the
owner's — do not pick, nudge, or tidy them into a recommendation.

**Second, and independent of that branch: bring `ECHO-UNQUALIFIED` down from
24.** Each one is a mention of a withdrawn figure's number in a document other
than its own with no disqualifier within 4,000 characters. `ci/check-figures.py`
prints the count on every green run; the sites are recoverable by iterating
`check_echoes` (see the probe pattern in this branch's history). Some are
genuine — a document stating the withdrawal in its own words rather than the
ledger's `disqualifier` wording — and those want the wording aligned, not the
prose rewritten. **Lower the ceiling as you fix them.**
