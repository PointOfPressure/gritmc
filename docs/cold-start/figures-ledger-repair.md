# COLD START — `figures-ledger-repair`

### COLD START — branch `figures-ledger-repair`, off `main` `b94f3ebf` then rebased onto `4674c952`, 2026-08-18, area "the figures-and-docs ledger"

**Every published number, its window, and its floor.** Worktree
`~/grit/work-fig`. No corpus lock taken, no build, no jar, no box time.

## WHAT MOVED

    figures registered            93  ->  103
    provenance naming no commit   59  ->  0
    ledger rules                  15  ->  20   (16-20, plus a bench falsifier set)
    check-figures-falsify cases   13  ->  40
    corpus-floor-falsify cases    36  ->  39
    dashboard rows unregistered    5  ->  3    (measured for the first time)

## THE FIVE THINGS THE BRIEF NAMED, AND WHAT EACH TURNED OUT TO BE

**1. Bare-date provenance — 25 claimed, 59 real.** 24 of the 25 entered the
ledger in ONE commit, `5c5df53e`, the 2026-08-07 backfill written over figures
already published in the documents, so `git log -S` on a figure id dates the
bookkeeping and not the measurement. `ci/figure-provenance.py` runs rule 1
backwards instead — the first commit in which a figure's claim was true in its
own doc — and reports EVERY witness (residual, denominator, claim,
artefact_claim) rather than one, because a single token does not fail loudly, it
answers wrongly: `bench-r7-rustmc-cpu`'s `artefact_claim` is `BENCH_FLAVOURS`, a
variable name, and searching on it returns a bench run from five days earlier
with different numbers. Writing the rule then found **34 more** entries whose
provenance is a rich paragraph naming a branch, a date and a machine but no
tree. All 103 now name a commit `git cat-file` resolves.

Two of the 25 appear to contradict their recorded date and neither does:
`ow-r16-n2-prechambers` is dated 08-03 and its commit landed at **00:29** on the
4th; `rss-shipped-ten-players` is dated 08-06 and its commit landed at **22:43**
on the 5th. One working session either side of midnight, both times. That is the
argument for shas in one line.

**2. `REQUIRED_WINDOW["bench"] = []` — the comment is TRUE, and the defect is
elsewhere.** The brief expected an unfalsifiable check. The disjunction it
promises (radius+chunks OR level+view_distance) is really implemented and really
fires: four new cases prove it reds on neither arm, on half the generation arm
and on half the running-server arm, and stays green on the running-server arm
alone — which is a control, because `boot-superflat` and the three `rss-*`
entries carry no radius at all and a "tightening" that moved radius+chunks into
the required list would red four honest entries. **What the rule does not reach
is the pool**, and that is rule 17.

**3. Worker count inherited and unrecorded — and it is the CPU table.**
`ci/bench-servers.sh` named `RUSTMC_WORKERS` in a comment eighty lines above the
code and set it nowhere, so the width came from physical cores unless the
invoking shell had exported it. `results.tsv`'s `peak_cores` is an observation,
not the setting: a pool of 24 that peaked at 11 writes what a pool of 12 that
peaked at 11 writes. `pipeline::worker_threads`'s own doc prices the axis at
**78.54 / 82.69 / 98.48 CPU-s at 1 / 12 / 24 threads**, identical build, same
810 G instructions to within 0.2% — 25.4% on a figure published to six
significant figures. Now pinned from sysfs topology, exported so the binary uses
the value we recorded, and stamped into a `window.tsv` with the commit and
whether the tree was dirty. It resolves to 12 here, which is what the binary
already chose, so **no published number moves**.

The JVMs stay unpinned deliberately and that is now stated: `-Dmax.bg.threads`
would make the reference a modified vanilla, which is the one thing the launch
headline rests on.

**4. `docs/PARITY.md` §2 had no floor column — and three of twelve rows are
UNDER their floor.** overworld 31415926 r=16: 39,143 against 49,570. nether
31415926 raw: 39,941 against 95,240. nether 20260722 r=16: **254,799 against
317,725** — the row that has been quoted as this project's worst number is not
evidence of anything. The column also rescues the best row: the End at r=16 is 0
against a floor of 252, which printed bare read like the zero of an unmeasured
window. Four rows say `not measured` rather than `—`, which the old shape could
not distinguish from "no core".

**5. Two ensembles, one declared list — the shape was right, the names had
moved, and the current form is worse than two.** Main's standing floor is
`coverage92-selfcheck-n6-20260811` and case 1's arming was corrected on
2026-08-11, so most of what the brief described is already fixed.
`ci/checks/gameplay-unstable.toml` was not: it held rates from **three**
ensembles with nothing machine-readable saying which was which — four entries
from an unnamed N=26 sweep on main `92f1658` against a corpus of 61, one from
`twin-homog-20260809`, adjudicated against a floor over a corpus of 92.

The live disagreement that hid there:
`a-player-that-leaves-the-ground-is-resynced` reads **8/15** in that file and
**5/15** in the floor file — same scenario, same N=6, same 15 pairs, two
ensembles. Neither is wrong; neither said which ensemble it was.

**It was not cosmetic.** `cmd_adjudicate`'s EVIDENCE DOES NOT REPRODUCE check is
guarded on `evidence.distinct_projections`, and with no `[evidence]` table at
all it was unreachable on **five entries out of five** — switched off by a field
nobody supplied rather than by anyone deciding to switch it off.

## THE JUDGEMENT, WHICH THE BRIEF ASKED FOR RATHER THAN A MEASUREMENT

**Keep `water-spreads` in `gameplay-unstable.toml`. Its removal from
`gameplay-baseline.toml` was also right. The two files differ because one is a
gate input and one is not.**

The scenario has been parked in `ci/checks/gameplay/open/` since 2026-08-10 and
is loaded by nothing, so no current ensemble measures it and the floor file's
evidence does not list it — **that absence is the scenario being parked, not the
scenario stabilising**, and reading it the other way is the trap. Deleting the
entry would destroy 24/25 at N=26, which cannot be re-taken at the N=6 the
current ensembles run and would read afterwards as a scenario nobody had ever
found unstable. `gameplay-baseline.toml` removed the same name on the same day
and was right to: a name in a GATE INPUT that no scenario answers to is a line
nobody can act on. `ci/corpus-floor-falsify.sh` case 11 pins that the gate does
not read the unstable file, and that pin is the whole distinction.

So the stale-vocabulary / current-vocabulary choice is not a choice between
files. Both are kept, both now say which ensemble they describe, and UNSTAMPED
and UNVERIFIABLE make it impossible to add a third that does not.

## THE FIVE NEW RULES

* **16 FLOOR-NOT-COMPARABLE** — landed with `docs-figures-ledger`, renumbered
  here when both branches claimed 14/15.
* **17 BENCH-WORKERS-UNRECORDED** — a ratchet at 8. `"unrecorded"` is honest and
  those eight cannot be repaired by editing the file, only by re-taking the run.
* **18 PROVENANCE-NO-COMMIT** — asks git rather than matching a pattern, which
  is the half that earns its keep: `20260812` is a date wearing hex's costume
  and sits inside four live provenances, and a mistyped sha reads exactly like a
  copied one.
* **19 DASHBOARD-UNREGISTERED** — a ratchet at 3. `[coverage] pattern` matches
  `[0-9]+[.][0-9]{4,}%` and the dashboard is made of counts, so the scan
  guarding every percentage in seven documents could not see one number in it.
* **20 STAMP-DISAGREES** — `window.scenarios` must equal the `corpus=N` the
  artefact stamps. **Eleven entries cited one run and described another**: five
  said 93 against a stamp of 92, six said 93 against a stamp of 106.
  `docs/PARITY-ROADMAP.md` 4479 states the correction outright — "The
  denominator is 92, and every document that led here said 93" — and it reached
  the documents and not the ledger.

## THINGS THAT CAUGHT ME, WHICH IS THE USEFUL PART

* **`--check` in my own provenance tool was VACUOUS.** It selected the same set
  as the default listing — entries with no sha — so it went green over an empty
  set in 0.078 s the moment the last one was filled in. In the script written to
  close a different instance of exactly that. The obvious fix is wrong too and
  was measured: re-deriving every figure reds 43 of 103, because a hand-written
  provenance often names the branch's BASE while the derivation names the
  publishing commit. The set is the 56 entries the tool wrote; an empty set now
  exits 2.
* **It then failed on its first honest run**, on an entry written an hour
  earlier. `nether-r7-basalt-raw`'s provenance named `02d450e7`, copied from its
  sibling core. The raw number was MEASURED at `a5f703d0`, eight commits earlier
  the same day; `02d450e7` is where it was WITHDRAWN. Right provenance for the
  core, wrong one for the raw half.
* **SELF-ARTEFACT (ratchet 7) caught my two new entries** pointing `artefact` at
  their own `doc`. Re-pointed at `docs/WORKLIST.md`'s raw-and-core table.
* **DOC-DRIFT caught my own documentation commit**: writing `+7.03%` into
  `docs/BENCH.md` a second time reds a `claim_count` of 1.

## NULLS — DO NOT RE-RUN THESE

* **The bench regression is NOT stalled and `docs/BENCH.md` §3 is correct.** The
  brief says the item stalled for a fortnight for want of a baseline sha; the
  attribution ran on 2026-08-09 and was written up on 2026-08-17. What was
  missing is that none of it was in the ledger — now four entries, and
  `ci/bench-attrib-20260809.txt` generated from the logs' PRIMARY blocks. The
  open item is real and unchanged: **~14 points landed on top of the chambers
  cost between `9b4afd4e` and `35b3bde5` and nothing has attributed it.**
* **Do not quote the attribution logs directly.** Each carries both metrics and
  the secondary CPU-s block reads **+18.11%** where the primary reads +7.03%.
  All twelve runs shared the box (7-13 competing processes, the harness's own
  `contend=` column), so CPU-s is contaminated and instructions retired is not.
* **`ci/corpus-floor-falsify.sh` case 1 is already armed correctly.** The
  ensemble-AND-projection match landed 2026-08-11; the leftover-state hazard the
  brief describes was fixed before this branch started.
* **The `[coverage]` docs list is right to exclude `docs/BENCH.md` and
  `docs/WORKLIST.md`** (74 and 92 percentages, 73 and 71 unregistered). Adding
  either is a re-measurement programme. Registering §3's four headline figures
  individually is the shape that works, and is what was done.
* **`cited-audits.toml` rows have an expiry.** When
  `corpus-projection-audit` and `server-unported-audit` land, delete the row and
  re-point the ledger entry's `artefact` at the merged file. It is scaffolding,
  not a second permanent home for a number.
