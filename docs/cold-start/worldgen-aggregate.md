# Cold start — branch `worldgen-aggregate`

### COLD START — branch `worldgen-aggregate`, 2026-08-12

**Docs and ledger only. No generator source was touched, no vanilla reference
was generated, no diff was run, and the corpus lock was never taken.** The
whole item is a strict reading of `ci/figures.toml` plus one free re-read of a
`.pos` matrix that already existed.

#### What the item asked, and what the tree said instead

Briefed on `RESUME-STATE`'s standing headline: *the project's one quotable
worldgen exactness figure fell below its own floor, leaving no aggregate parity
percentage — and that is still true.* **The first half is history.** That
headline was written on tip `8c7e024c` where seed 12345 read 1,329 against a
floor of 141..2,241 at N=15 (0.59x). Commit `c8178dc7` — DETERMINISM §24 —
widened the ensemble to a pool of 40 and the window re-anchored at **1,279 of
107,053,056, 99.998805%, floor 0..362 at N=39, 3.53x**. The axis has had a
headline for two days and the state file did not know.

The second half is true, for reasons the state file did not give.

#### The inventory (DETERMINISM §55.2)

47 worldgen-axis figures. Twelve carry `quotable = true` **and** a percentage —
what a consumer enumerating the ledger mechanically picks up. **Two are current
exactness levels of this port against unmodified vanilla:**
`ow-r16-12345-level-n39-iceberg` (99.998805%) and `end-r16-n6-exact`
(100.000000%). The other ten break down as four superseded rungs of the same
window, one stale by its own provenance, one restating a figure disqualified
for uncontrolled chunk order, two that **are floors** (vanilla against itself,
carrying `percent` fields — quoting either publishes vanilla's
irreproducibility as our fidelity, and the nether one does it at 99.1%), and
two r=7 cores whose windows say `harvest = unrecorded`.

Three ledger-mechanism findings worth keeping:

* **`quotable = true` on a restatement means the restatement is faithful, not
  that the number may be quoted.** `readme-restates-ow-r16-admissible` was
  `true` while its source `ow-r16-admissible-n2` is `false` for uncontrolled
  chunk order. The README prose is correct; the flag was not. The window-match
  rule could not catch it because that entry's `compare_keys` **omitted
  `dump_order`** — the key the source was disqualified on. **SUPERSEDED
  2026-08-16: `main` fixed both halves** (rules 9 and 10), that entry is now
  `quotable = false` and compares all eleven of its source's keys, and rule 10
  then caught this branch's own restatement — see the 2026-08-16 entry below.
* **`RESTACKED ... NOT RE-MEASURED` is free text.** It appears 7 times in
  `ci/figures.toml`. `ci/check-figures.py` never reads `provenance`; there is
  no staleness notion in the checker at all.
* **The gameplay projection move to `67604e5eb262728f` cannot touch worldgen.**
  `projection` is a window key on `corpus-floor`/`corpus-reach` kinds only; no
  worldgen figure has one and `REQUIRED_WINDOW` gives worldgen no such field.
  Checked rather than assumed.

#### The aggregate, refused twice (DETERMINISM §55.5)

| composition | residual | denominator | % | summed floor | verdict |
|---|---|---|---|---|---|
| as the ledger stands, mixed N | 80,613 | 213,339,370 | 99.962214% | 767,104 | **9.52x under** |
| one estimator, common N=5 | 97,044 | 214,106,112 | 99.954675% | 116,102 | **1.20x under** |

99.92% of the first floor is the nether at 16.4% of the window. Both are now
registered `quotable = false` so the next agent to do this very easy arithmetic
finds it already refused rather than finding a respectable-looking 99.96%.
Three further reasons no aggregate is defensible even if it cleared its floor,
none fixed by references: the **seeds differ** (12345 vs 20260722); the **End's
denominator is ensemble-masked** (71,368,452) where the overworld's is not; and
**sum-of-floors is only a floor if one estimator at one N produced all of it**.

#### The finding: two floor families face opposite ways in N (§55.4)

`nether-r16-n6-matched` records *there is no N at which it would carry a level
claim: the floor grows faster than the residual falls.* That is a true statement
about a **union** — `ci/floor-ensemble.sh`, `ci/floor-ladder.py`, which
accumulate `union over j>=2 of pos(ref1, refj)` and can only grow — and **every
r=16 nether floor in the ledger came from it** (317,725 / 668,532 / 766,490 /
1,172,835). The **innocent leave-one-out** family
(`ci/window-resolving-power.py`, `ci/floor-census.py`, `ci/floor-anchor.py`)
charges a held-out vanilla run against the spellings the others produce and is
monotonically non-increasing in N; **every seed-12345 overworld floor came from
it** (4,835 / 2,241 / 362). The 766,490-against-362 gap is **estimator, not
world.**

**DETERMINISM §4.4 already published the nether on the correct estimator.** Only
the union column was carried into the ledger and the roadmap. `ci/nether-anchor-floor.sh`
reproduces §4.4's N=5 row to the block from a different construction
(`2n-3` anchor diffs, `--verify-against` the full 30-diff matrix,
candidate-by-candidate) and adds one refinement: §4.4's **85,181 is the best of
the six 5-subsets, 92,226 the worst**, so against a worst-case floor the honest
pairing is the median **89,104 — 0.80x, not 0.77x**.

(Registered unquotable, both of them: the mixed-N aggregate because its **floor exceeds its residual**, and the nether's 89,104 as **corroboration** only — neither may be quoted as an exactness.)

#### What I killed as a null — do not re-run these

* **Do not re-derive the nether floor.** It is done, twice, by two
  constructions, and `ci/nether-anchor-floor.sh` re-reads it in about a minute
  from `/home/sayer/rp-logs-ladder/nether/pos` with no generation.
* **Do not extrapolate the nether ladder from the rungs that exist.** I tried;
  it does not survive. Four rungs, margins 0.73 / 0.56 / 0.62 / 0.77 with no
  trend. A power fit over all four gives `floor_max ~ N^-0.930` against
  `charge ~ N^-0.904` and says **N ~= 2.9e7**; the same fit without the N=2 rung
  gives `N^-1.110` against `N^-0.515` and says **N ~= 8**. One rung, seven
  orders of magnitude. Neither number means anything and neither should be
  quoted.
* **Do not pool the two `rows_rev` nether references into the `rows` pool to
  get cheap rungs.** §4.4 prices the harvest-order term at 1.85x at N=2 on this
  window, and `ow-r16-20260722-floor-mixed-order` is the registered case of a
  mixed pool making a floor strictly *worse* (max rises 8,010 blocks).
* **Do not reach for a sub-window or a y-band to escape the nether floor.**
  Both registered as unquotable for selecting on the runs that validate them;
  the y-band version is the null in §24.4 and fails in the informative
  direction.

#### The single next item, and the result that closes it

**Six more `rows` references for the r=16 nether — about seven minutes of boots
plus ~20 anchor diffs, call it half an hour of box time — to get rungs at
N=6..11.** §4.5 prices a nether r=16 reference at ~70 s and the box holds eight
(6 `rows`, 2 `rows_rev`).

It closes either way and the two outcomes are opposite:

* **the margin climbs** → the nether is on the seed-12345 trajectory, and the
  axis knows what buying the dimension costs; or
* **it stays flat near 0.67x** → our charge and the floor descend together, the
  r=16 nether is *permanently* unresolvable by references, and the axis should
  stop spending on it. The correct nether statement is then the
  indistinguishability claim — *over 35,684,352 blocks the admissible
  instrument cannot separate this server from an unmodified vanilla run of the
  same seed* — and never a percentage.

The aggregate stays refused under both. What changes is whether the refusal is
provisional or final.

#### Files

`docs/DETERMINISM.md` §46 (+ `docs/claims/DETERMINISM-46-worldgen-aggregate.toml`),
`ci/nether-anchor-floor.sh`, five new ledger ids
(`nether-r16-20260722-anchored-floor-n5`,
`nether-r16-20260722-anchored-charge-n5`,
`worldgen-aggregate-asledger-n-mixed`, `worldgen-aggregate-n5`,
`parity-restates-ow-r16-12345-n39`), and a correction to `docs/PARITY.md` §2
whose ✅ paragraph still called the superseded 69,288 *the only quotable
exactness figure any r=16 overworld window carries*.

---

### COLD START — branch `worldgen-aggregate`, 2026-08-16

Tree-only repair turn. **No generation, no build, no box, no corpus lock, and
none was wanted** — every fault was in the branch's own docs and ledger. Three
faults, all four tree-only T0 checks green at the end:
`ci/check-doc-claims.py`, `ci/check-figures.py`,
`ci/corpus-falsifiable.py --check` (106/106), `ci/liveness-proofs.py --check`
(5/5).

#### 1. DETERMINISM §35 -> §33 -> §46 — a claim collision, twice

> **SUPERSEDED IN PART, 2026-08-18, branch `worldgen-land`.** The 35 -> 33 move
> recorded below was correct against the `main` it was taken on and is left
> standing as written. It did not survive contact with the rest of the lane:
> `worldgen-isolated-drift` also numbered its section 33, and its 33 is the one
> that answers §32.3's open question and is forward-referenced from §32's own
> text. This section is now **§46**, which is the first number free on every
> branch in flight — 36..39 are claimed by `clamped-floor-probe` and
> `regiondiff-live-fixture`, 40..45 by `worldgen-be-save-arms`. Everything
> below about WHY a renumber is cheap and what rebounds with it still applies;
> only the destination changed.


`main` owns §35 (`worldgen-deliver-once`, `delivered-once-never-written-again`).
This branch moved to the lowest free section, **33**. Renumbered by grepping the
old number rather than by working a list: the `## 33.` heading, six `### 33.N`
subheadings and ten in-section `§33.N` references in `docs/DETERMINISM.md`; the
claim file; two references in `ci/figures.toml`; two in `docs/PARITY.md`; one in
`ci/nether-anchor-floor.sh`; five in this file.

**The part worth remembering is what the grep found that the list did not.**
`worldgen-deliver-once` was evidently written expecting §33 and landed as §35,
leaving two references on `main` pointing at a §33 that did not exist —
`ci/figures.toml:644` (`§55.6`, explaining the End-window re-run, which is
§35.6) and `ci/checks/test-targets.toml:604` (`§55.4`, citing the 81-against-155
carve counts, which are §35.4). Taking 33 would have silently rebound both to
this branch's section. Both were repaired to §35 in the same commit, because a
reference that resolves to the **wrong** place is worse than one that resolves
nowhere, and this renumber is what would have made them wrong.

#### 2. `end-r16-n6-exact` claim_count 7 -> 8 — legitimate, not a duplication

The eighth `100.000000%` in `docs/DETERMINISM.md` is §55.2's inventory row
naming the figure by its ledger id with its percent, residual, floor and N.
**Checked rather than assumed, because a whole-file duplication of exactly this
kind corrupted another branch:** `main` holds exactly seven occurrences, this
branch's entire diff to that document is one appended section, and the eighth is
that row. A new *statement* of the same measurement, which is what an inventory
is for. None of the figure's numbers moved — 100.000000%, residual 0,
denominator 71,368,452, floor 252, N=6, as taken 2026-08-06.

#### 3. `parity-restates-ow-r16-12345-n39` — the same measurement, so compare on all twelve

Rule 10 (`RESTATED-WINDOW`) refused it for comparing 10 of its source's 12
window keys, `order` and `p_hide` omitted. **The restatement IS the same
measurement, and that was established by reading before anything was widened:**
all twelve keys are byte-identical to `ow-r16-12345-level-n39-iceberg` —
`order = vanilla-first` and `p_hide = 7.4%` included — as are `residual` 1,279,
`denominator` 107,053,056, `percent` 99.998805%, `floor` 362, `pair` and
`baseline`, and the provenance records no re-measurement.

So the fix is **completing the comparison, not exempting the keys**. Had `order`
differed it would have been a different measurement wearing the same number and
the entry would have been withdrawn instead; the rule's exemptions were not
touched.

**Falsified rather than asserted — three injections, all red, control green:**

```
control  (unmodified)                                        FIGURES GREEN
A  order   vanilla-first -> ours-first    RED [WINDOW-MATCH]   order mismatch
B  p_hide  7.4%          -> 18.1%         RED [WINDOW-MATCH]   p_hide mismatch
C  compare_keys reverted to the ten       RED [RESTATED-WINDOW] 10 of 12
```

A and B are the point: before this change both keys were *unreadable* by the
check, so neither injection could have moved it. The two keys are now
load-bearing.

#### Doc corrections this turn forced

§55.2's paragraph on `readme-restates-ow-r16-admissible` described a state
`main` had already left: that entry is `quotable = false` today and compares all
eleven of its source's keys, so "the flag and the prose disagree" and "the
window-match rule cannot catch it" were both stale. Rewritten as the after
picture, keeping the worked example — and noting that rule 10 then caught the
identical shape one day later in this branch's own restatement, written by an
author who had just described the fault.

#### What is NOT changed, and must not be re-litigated

No measurement was re-taken and no figure's value moved this turn. The
aggregate stays refused, both entries stay `quotable = false`, and the single
next item is unchanged — the nether margin at N=6..11, above.
