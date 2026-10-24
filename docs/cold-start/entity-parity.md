# COLD START — branch `entity-parity`

Append-only. Newest entry last.

### COLD START — branch `entity-parity`, 2026-08-18, what entity parity is measurable to

Worktree `~/grit/work-ent`. **Everything below is tree-only or reads recordings
that already existed, except one probe run that takes no corpus lock.** Read
`docs/PARITY-ROADMAP.md` §53 for the reasoning; this file is the resumable
state.

## READ THIS FIRST: three worries came in, two are real

* **The corpus reaches almost no entity.** Real. 4 of 158 jar types have ever
  been on the wire, 11 are declared after this branch.
* **The whole corpus floor is entity non-determinism.** Real, and nobody had
  said it. `floor = 4` and all four names are entity scenarios.
* **`observe_from` hides entity behaviour from 102 of 115 checks.** **Not true
  of this server**, and the count is the wrong shape anyway: 11 of the 13
  checks that keep BOTH clients are entity checks. Measured over 61 paired recordings: no entity kind is
  actor-only on our side and both-sided on vanilla's. Do not re-open this;
  `ci/entity-broadcast-symmetry.py` is the instrument and its header carries
  the one row that looks like a counterexample and is not.

## The instruments this branch adds, and what each refuses

| tool | question | gates? |
|---|---|---|
| `ci/entity-reach.py` | which of the jar's 158 types the corpus reaches and can GRADE | **yes**, T0, with `ci/entity-reach-falsify.sh` (11 assertions) |
| `ci/entity-leak-audit.py` | how far a spawned entity leaks, and how much of it `within` can place | no |
| `ci/entity-broadcast-symmetry.py` | does any green rest on the discarded client | no |
| `ci/entity-movement-report.py` | is `add_entity.movement` reproducible, per type | no |
| `ci/corpus-dead-observe.py` | kinds a check declares that vanilla never sends | no |
| `ci/floor-stamp-vocabulary.py` | the floor's missing `vocabulary` | **yes**, T0, with a 7-assertion falsifier |

Four of the six print and never gate, and each says in its header why. Three of
them read raw traces **without the projection**, which is sound for those three
and only because none of them compares two streams — they are presence and
count censuses of one server at a time.
`docs/cold-start/corpus-projection-audit.md` (on `origin/corpus-projection-audit`)
is about the other kind, where unprojected screens produced hits that dissolved.

## Killed as nulls — do not re-run these

* **`observe_from` is not hiding a broadcast bug.** 38 `(scenario, kind)` pairs
  where we emit nothing to anyone, 18 both-sided on both servers, 1 flagged and
  explained. The first version of the script tested PRESENCE and lit up six of
  seven scenarios, every one of them a case where our actor count was zero too.
  A ratio, not a presence.
* **`damage_event` and `set_health` are not a server work item.** Zero
  appearances in 64 recordings. `Player.hurtServer` offset 31 returns before
  either is sent, because the substrate is creative and
  `Abilities.invulnerable` is set. Needs a **survival player** — a substrate
  change, the same class as `Step::FinishDig`.
* **The four disputed `add_entity` fields are settled and the dispute is
  stale.** The standing floor's own ensemble carries 95 pairs in all six runs
  including all four, and drifts from today's recorder in neither direction.
  The 91-pair ensemble is `twin-homog-20260809`, superseded on 2026-08-10.
  §50.5's worry about the two container-throw scenarios goes with it.
* **A type-keyed exemption for mob spawn yaw is still the wrong fix.** §30's
  null, re-confirmed by the floor: a pig's yaw is random only on the
  `EntityType.create` path and the wire does not carry which path spawned it.
* **Do not widen `compare::near`.** 0 of 4,496 leaked observations carry a
  position. There is nothing to widen — a delta-encoded move does not say where
  it happened. Scenario ORDER is the whole mitigation.

## What is deliberately NOT done, and why

* **`add_entity.movement` is named AND narrowed — this was done.** N = 6 cold
  vanilla servers: `minecraft:item` is `drawn` (6 distinct), and nine types
  across four spawn routes are `constant [0.0, 0.0, 0.0]`, including the pig,
  whose YAW is drawn and whose MOVEMENT is not. The drop is now two types wide
  (`item`, `player`); `player` is untested by this probe and keeps its
  exemption on the ledger's reason alone. The narrowing rests on the VALUE
  being the identity element across four routes, not on the count — `p_hide` at
  N = 6 is 45.1% and the report says so.
* **`ci/checks/projection-drops.toml` is untouched.** It is on
  `origin/corpus-projection-audit`, not on main. **When either branch lands
  that ledger needs `add_entity.movement`'s `stage` moved `decode -> project`**,
  and its observer counts move with the entity family: `add_entity` 7 -> 11,
  `sound` 6 -> 10, `entity_position` 2 -> 3, `block_update` 74 -> 80. Measured
  by running that branch's own `ci/projection-drops.py` against this tree.
* **`080-spawn-egg-makes-a-mob` is not renumbered.** It leaves a live mob at
  check 80 of 115 and the live-mob ordering rule does not cover it. Moving it
  would renumber a third of the corpus; it is declared `residue = "walks"` in
  `ci/checks/entity-reach.toml` instead, with its measured reach.

## The numbering trap, because it has now cost four renumbers

`ci/checks/gameplay/`'s number is a **shared namespace across every branch**,
and `origin/main` is not the whole of it. The golems moved `730`-`737` ->
`773`-`780` -> `781`/`782` -> `788`/`789`, and the third collision would have
inserted a live wandering mob between `781` and `782`, which are a matched
control pair. Before taking a number:

```sh
for b in $(git branch -r | sed 's/^ *//' | grep -v HEAD); do
    git ls-tree --name-only "$b" ci/checks/gameplay/
done | sed 's|.*/||' | sort -u
```

Renumber the newcomer, never `790` — that is
`docs/cold-start/corpus-item-registry.md`'s ruling of 2026-08-17 and it is
right: `790` is the anchor of a rule quoted in `docs/SERVER.md`,
`tools/oracle-runner/src/compare.rs`, three cold-start files and the corpus
README.

## What the probe run also settled, unasked

**§30.5's A/B table had a PENDING row and this branch filled it.** With the
snow golem out of the corpus the copper golem goes **5/15 -> 0/15** and the
iron golem stays 0/15, while the positive control still flaps 15/15 — so §30.4
was right that the snow trail was the cause, and it is not a statement about a
quiet box. Two variables moved rather than one (the corpus is 10 scenarios, and
the projection now compares `movement`), and the second makes the result
stronger: a projection comparing MORE cannot make a scenario flap less.

## The single next item

**`minecraft:player`'s `movement` is the last untested half of the drop.** The
entity probe has no scenario that announces a player, so the ledger's other
claimed draw — a tick-phase coin toss on which tick vanilla's last motion
packet went out — has never been read off the wire, and the type keeps its
exemption on the strength of a paragraph. Add `530-walking-away-drops-a-standing-player-from-view`
to `ENTITY_PROBE_EXTRA` and run `ci/entity-spawn-probe.sh` at N = 6; it takes no
corpus lock and costs about twenty minutes. If `minecraft:player` reads
`constant`, the drop closes entirely and `ci/checks/projection-drops.toml` loses
a row rather than gaining a narrower one.
