### COLD START — branch `worldgen-land`, off `main` (`4674c952`), 2026-08-18

**The worldgen backlog is landed. Twelve branches carried work that was not on
`main`; eight of them had unique commits and all eight are replayed here, 115
commits, with two branch-against-branch contradictions resolved by judgement
rather than by union.** Worktree `~/grit/work-wgland`, remote dir
`~/rustmc-wgland`. Not merged — the chain merges.

This is a LANDING branch. Nothing here was re-derived and no window was
re-taken. Where a number appears below it is quoted from the branch that
measured it.

---

## WHAT LANDED, IN THE ORDER IT WAS REPLAYED

| branch | commits | how it went |
| --- | --- | --- |
| `worldgen-spruce-leaves` | 5 | clean |
| `worldgen-isolated-drift` | 10 | clean; `merge.appendonly` resolved DETERMINISM every time |
| `worldgen-sulfur-r18` | 3 | clean |
| `worldgen-be-save-arms` | 61 | clean |
| `worldgen-aggregate` | 6 | 2 conflicts, both `claim_count` arithmetic |
| `village-pool-elements` | 11 of 12 | one commit dropped, one file declined — see below |
| `worldgen-pipeline-parallel` | 5 | clean |
| `worldgen-perf-unattributed-14-decide` | 11 | clean |

**`worldgen-be-save-arms` is the whole deep stack.** It contains
`worldgen-jigsaw-random` (51) as an ancestor and `worldgen-block-entities`
(23) patch-for-patch — `git cherry origin/worldgen-be-save-arms
origin/worldgen-jigsaw-random` is empty and all 23 of block-entities' commits
show `-` against jigsaw-random. Landing the tip lands all three. Do not land
the other two; they are not separate work.

Same shape, smaller: `worldgen-perf-unattributed-14-decide` contains
`worldgen-perf-unattributed-14`'s single commit patch-equivalently.

## WHAT HAD NOTHING TO LAND — AND THIS IS NOT THE SAME AS BEING EMPTY

* **`chunk-order-residual` reads 15 commits ahead of `main` and is already
  fully on it.** Every one of the 15 is patch-equivalent to a commit `main`
  already carries under a different sha — `git cherry origin/main
  origin/chunk-order-residual` returns 15 lines and every one of them is `-`.
  The branch was landed by rebase at some point and the ref left behind.
  **Do not re-land it and do not read its depth as backlog.** `git rev-list
  --count` cannot tell the difference; `git cherry` can, and it is the only
  thing that should be used to size this backlog.
* **`worldgen-12345-next-items` reads 75 commits ahead ONLY IF you ask the
  local ref.** `refs/heads` is stale at 2026-08-10; `refs/remotes/origin` is
  0 ahead. Ask origin. Every depth in this file was taken against origin.
* 21 further `worldgen-*` / `structures-*` / `village-*` branches are at 0
  ahead of `main` and are landed refs awaiting deletion, not work.

## THE TWO CONTRADICTIONS, AND HOW THEY WERE DECIDED

### 1. `worldgen-isolated-drift` and `worldgen-aggregate` both numbered a section 33

Not cosmetic. `ci/check-doc-claims.py` refuses it twice — `DUPLICATE-CLAIM
DETERMINISM §33 claimed by 2 files` and `DUPLICATE-HEADING DETERMINISM §33
appears 2 times`. Verified by restoring the collision after fixing it, so the
green is known to be able to go red.

**Isolated-drift keeps 33.** Its section is the ANSWER to §32.3's open
question about the unexplained ~200 blocks and it rewrites §32's own text to
point at that number. The aggregate is a standalone survey with no adjacency
to lose. Checked rather than assumed: `git show origin/main:docs/DETERMINISM.md
| grep §33` is EMPTY, so neither had a prior claim from `main` and the tie is
broken on what the sections are.

**The aggregate went to 46, not 36.** It had already moved once, 35 -> 33, to
dodge main's 35 — 33 was simply the next free number it could see. Free on
`main` is not free in the lane: 36..39 are claimed by `clamped-floor-probe`
and `regiondiff-live-fixture`, both live in other lanes, and 40..45 by
`worldgen-be-save-arms`. **If you renumber anything in this document, sweep
every branch and not just `main`, or you hand your problem to somebody else.**

### 2. `village-pool-elements` and `worldgen-jigsaw-random` rewrote the same placement path in opposite directions

This is the expensive one and it is a genuine either/or.

* `village-pool-elements` RUNS a village's `feature_pool_element`s on the
  structure's piece stream. Its signature is
  `random: &mut WorldgenRandom<Xoroshiro>` — mandatory.
* `worldgen-jigsaw-random` REFUSES them, records the refusal through an audit,
  and measures what the refusal costs. Its signature is
  `random: Option<&mut dyn RandomSource>` — `None` at the default.

They cannot both land. A feature element spends its draws on the same
`RandomSource` the containers take their `LootTableSeed` from, so an element
that runs and an element that is refused leave every later container in that
start at a different stream position.

**The jigsaw shape landed.** In order of weight:

1. **§44's accounting is taken with the elements refused** — `payload` 8 -> 0,
   `exact` 121 -> 129, village desync 1 draw of 292. Landing the other half
   turns all of those into statements about a build that no longer exists,
   silently, without re-taking one of them.
2. **The other half turns `RUSTMC_JIGSAW_STREAM` on for everybody.** That knob
   is global and its default is the owner's call. It is the same call from
   either side of the code, and a landing agent does not get to make it by
   choosing a function signature.
3. It is the later of the two and was written knowing about the elements — its
   own comment names the 13 village pools that hold one.

**What still landed from that branch, which is most of it:** `block_pile` and
`rotated_block_provider` ported into `rustmc-worldgen` with their own tests,
`golden_decorate`'s `Region` as a `BlockPileWorld`, and the census correction
in full — 5 feature elements and not 22, the kind table 2x over, and the 188
a block-kind count of which only 99 was ever village elements.

**What was dropped:** `fa507947`, which bundles the arguments of the signature
that did not land and touches nothing else.

**Three of that branch's own claims became false on landing and are corrected
here**, because a doc describing the resolution that did not happen is worse
than no doc: the `generate.rs` caveat that read "`feature_pool_element`s ARE
placed", the `docs/PARITY.md` row marked CLOSED, and the
`WIDE-WINDOW-R16-ALL-DIMENSIONS.md` "closed 2026-08-10". The COUNT corrections
in all three are kept; the closes are not. `RUSTMC_VILLAGE_FEATURE_SKIP` now
has no consumer and its WORKLIST entry says so in its first line rather than
describing a knob that does nothing.

## THE ONE ARITHMETIC TRAP IN A REBASE THIS SIZE

`ci/figures.toml`'s `claim_count` is a COUNT OF OCCURRENCES IN A DOCUMENT, and
two branches bumping it from different bases produce increments that **add**.
Both obvious resolutions are wrong: taking either side's total under-counts.

`ow-r16-12345-floor-n15` was bumped 7 -> 8 by `worldgen-spruce-leaves` (§17.4
prints the floor) and 7 -> 8 again by `worldgen-aggregate` (its inventory
lists it). Both are real and the answer is 9. `end-r16-n6-exact` was authored
7 -> 8 against a `main` holding seven, two more landed ahead of it, and the
answer is 10.

**Do not reason about this field. `ci/check-figures.py` counts the document
and is the authority** — it refused 8 with `141..2,241 appears 9 time(s) in
docs/DETERMINISM.md, the ledger says 8`, which is how the 9 was found.

## WHAT WAS NOT DONE, DELIBERATELY

* **The +14% perf attribution was not measured and no box time was spent
  trying.** `worldgen-perf-unattributed-14-decide` lands with the run armed and
  the refusing loads recorded. It needs 1-min load <= 2.5 held 60 s and ~14 min
  of quiet box; the box did not go below 4.0 at any point during this landing.
  `docs/WORKLIST.md` has the performance track DEFERRED behind parity.
* **`RUSTMC_JIGSAW_STREAM` was not enabled.** The recommendation is landed, the
  default is not flipped. The trade as §44 states it: 60 positions and 0 block
  states against one wrong `LootTableSeed`, and the knob is global so
  "trial chambers only" was never on the menu.
* **No vanilla reference was overwritten and no harvest was run.** This branch
  took the corpus lock zero times.

## NEXT

The single next item is the owner's `RUSTMC_JIGSAW_STREAM` decision, because
three things are queued behind it and nothing else in this lane is: the
`village_savanna` (81,77) PARITY row reopened above, `village-pool-elements`'
99 order-stable blocks, and `RUSTMC_VILLAGE_FEATURE_SKIP` having a consumer
again. **If it is turned on, the Stream arm must be re-measured with the
feature elements running — §44.8's numbers were taken with them refused and do
not carry across that flip.**
