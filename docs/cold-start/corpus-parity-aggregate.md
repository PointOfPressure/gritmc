# COLD START — branch `corpus-parity-aggregate`

### COLD START — branch `corpus-parity-aggregate`, 2026-08-12

**The item:** the project is asked "how far from finished" and has no answer it
can defend. §21 built the denominators and refused to collapse them, because
the answer moves 55x between units. That refusal was right and is no longer
sufficient — a number will be quoted with or without us. Build one, print its
spread, enforce its refusals.

**Deliverable:** `ci/parity-aggregate.py`, `docs/PARITY-ROADMAP.md` §37, two
`parity-aggregate` figures in `ci/figures.toml` and the new kind in
`ci/check-figures.py`, artefact at `ci/parity-aggregate-20260812.txt`.

## What it measures

Nothing. **Not one number here is a new measurement** — no vanilla was booted,
no build ran, the box-wide corpus lock was never taken. `reach` comes from
`ci/vanilla-reach-census.py`'s ceilings and `match` from `oracle-runner`'s
corpus score, both already on disk. This is composition, and composition is
where the errors were.

```text
  parity = reach x match     over the 4 units with a known ceiling

  strict     10.16%    band  9.86 .. 10.73%
  declared   10.73%    band 10.41 .. 11.30%
  units       3.86% .. 21.54%   (5.6x)
  three-factor, §21.4's presence discount:  8.18%
```

Headline to quote: **8-10%, over four units, and never without the spread.**

## The five things that would have made it wrong

1. **Dividing by the registry.** §29's ceilings exist; use them. Worth a factor
   of 3 of the spread (17.3x -> 5.6x) and it does **not** close it.
2. **Averaging.** Pooled is the estimator a reader reaches for and its weights
   are the sizes of Mojang's registries — `sound_event` alone is 60.2% of the
   pooled weight. Swap that one ceiling for its registry size and pooled moves
   12.53% -> 2.49%. The median was picked by a drop-one jackknife (27.9% swing
   against 51.6% and 77.1%), and the jackknife runs on every invocation.
3. **Treating presence as coverage.** `reach x match` credits us for surface no
   check reads. The three-factor form prices it at 2 points.
4. **Assuming the halves measured the same vanilla.** The census names its
   recordings by path and the floor by sha256, so neither could see the other.
   They are joined by hash now. They did agree — this project had been getting
   that for free.
5. **Quoting the best-looking score.** `~/rustmc-stack`'s 81/93 is the newest
   and largest on the box and is refused: its comparator moved to
   `proj:f7fbb29926074663` and the standing floor was never re-measured under
   it. `~/rustmc-pot`'s 71/92 is the highest one coherent with the floor.

## Killed as a null — do not re-run these

* **A single defensible headline covering both axes.** Worldgen has match
  figures and **no reach census of any kind**, so `reach x match` is undefined
  rather than small. It is refused as a printed row, not omitted. Do not try to
  fold `docs/DETERMINISM.md` §17's 99.935277% into a gameplay number; it is
  §21.1's category error one level up.
* **A weighted sum with better weights.** Tried and abandoned: every weighting
  that is not the ceiling is a weighting of Mojang's enumeration choices. There
  is no behaviour-weighting available because nothing has censused behaviour —
  that IS the missing `block` ceiling, below.
* **Recovering the projection from the score artefact.** `oracle-runner compare
  --json` writes `declared`, `strict`, `outcomes`, `reference`, `subject` and
  **no comparator identity**. It cannot be recovered; it has to be supplied.
* **A floor for the aggregate.** `reach` is a census, not a comparison, so
  ORACLE rule 6 has no premise to fire on (§21.4). The per-run spread stands in
  for it in the low band. Do not go looking for one.

## Refuted from my brief

* The brief said the corpus denominator moved **92 -> 93**. The tree says main
  is at **93 scenario files today** and `origin/gameplay-sound-coverage` takes
  it to **98**. Read it off `ci/checks/gameplay/*.toml`, never from a brief.
* The brief's sound-row figures were wrong and are **superseded — do not quote
  them from this line or anywhere else.** The tree's are **1,712 unreachable by
  construction, ceiling 256, of 1,968 registered** (`origin/corpus-reach-ceilings`,
  §29 / §27.1), and those are the ones this section and §37 compose from.
* The brief said **"a duplicate-heading check now runs in T0"**. It does not.
  `ci/check-doc-claims.py` collects headings into a `set`, so two headings
  bearing the same number are deduplicated and invisible to it; its
  `DUPLICATE-CLAIM` rule is about two claim *files*, not two headings. The
  guard appears to be on `origin/ci-doc-heading-guard`, unmerged. **There is a
  live instance on main right now**: `docs/PARITY-ROADMAP.md` lines 5211 and
  5212 are two consecutive `## N.` headings with identical titles (`## 22.` and
  `## 26.`), left by the `corpus-dark-systems` renumber, and `## 22.` also
  legitimately exists at line 4893. T0 is green on it. Not fixed here — it is
  not this item's file to move and PARITY-ROADMAP is the highest-collision file
  in the repo — but it should be swept.
* The brief said **block entities gate 66 of 163 `useItemOn` rows** and implied
  they were the top lever. §23 records **68 of 163 refusing on every state**,
  which is a different claim, and `block_entity_type` ranks **third** here with
  a marginal lever of **exactly zero**.

## The single next item that closes this

**A ceiling for `block`.** Three registry rows — `block` (43/1196),
`block_state` (87/32366), `entity_type` (4/158) — are excluded for want of a
jar rule capping them, and they include the row §21.5 ranks **first** by
untouched surface. The composite is therefore silent about the largest system
on the work order, and no amount of work on the four surviving units fixes
that.

**The result that closes it:** `ci/vanilla-reach-census.py --json` emits a
non-null `registry.block.ceiling` with a stated jar rule and an owner, and
`ci/parity-aggregate.py` composes over five or more units. §29's `--explain`
names the two rules already tried and rejected; start there rather than
re-trying them.

Second, cheaper, and independent: **stamp the comparator into
`oracle-runner compare --json`.** One field. It removes `--binary`,
`BINARY-NEWER-THAN-REPORT` and a whole class of unprovable provenance, and it
is the field whose absence made §15's defect possible in the first place.

## How to re-run it

```bash
# needs a tree whose census emits `ceiling` — not main as of 3ddb3ca8
ci/vanilla-reach-census.py --json /tmp/census.json
ci/parity-aggregate.py --census /tmp/census.json \
    --report ~/rustmc-pot/oracle-work/gameplay/report.json \
    --binary ~/rustmc-pot/target/release/oracle-runner
ci/parity-aggregate.py --selftest      # 12 cases, green control at both ends
```

Exit **3** today, not 0: quotable **as of** 92 scenarios, not current against a
live corpus of 93. That is by design — §37.8.

**This branch depends on `origin/corpus-reach-ceilings` for its number but not
for its code.** On main the script refuses `[NO-CEILINGS]`, correctly, and that
refusal is one of the eight proved. It needs no rebase to be merged; it needs
the ceilings branch to have landed before the figure it registers can be
reproduced from the tree.
