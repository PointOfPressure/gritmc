### COLD START — branch `gameplay-sound-coverage`, off `main` (`40883b00`), 2026-08-12

**The item was §21's first-ranked one: 29 of 1,968 sound events ever heard.
The ceiling is measured, six scenarios are written and the corpus grew 92 ->
98.** Worktree `~/grit/work-sound`, remote dir `~/rustmc-sound`. Not merged.
Full account in `docs/PARITY-ROADMAP.md` §46.

**WHAT IS MEASURED AND WILL NOT NEED REDOING.**

* **This server can emit at most 256 of 1,968 sound events — 13.01%.**
  `crates/rustmc-server/src/sounds.rs` is the only emitter (the `cb::SOUND` at
  `world_send.rs:2552` is inside a unit test). Its whole vocabulary is 122
  distinct `*.place` events and 114 distinct `*.step` events, both reached
  through `data/vanilla/sound_types.json`, plus 21 hand-written names. **So
  1,712 of the 1,939 unheard events are `absent` by construction and no
  scenario can move them.** Do not write a check for one.
* **The 347 cheapest of those 1,712 are a server item, not a corpus item.**
  They are the `break`, `hit` and `fall` columns of the sound-type table this
  server already parses and already keys by block state. What is missing is
  the call site — a break sound on the creative dig path. One change converts
  347 events from unreachable to reachable, and it is worth more than any
  number of new scenarios. **This is the single next item.**
* **Step sounds cannot be checked and the measurement is done.** Over the six
  recordings of `coverage92-selfcheck-n6-20260811` the corpus records
  `block.grass.step` **35, 40, 34, 34, 35, 35** times — a 6-count spread on an
  unchanged corpus, because `Entity.playStepSound` fires off a distance
  accumulator a fixed-hop walk straddles. A step-sound scenario is
  reference-unstable by construction. **Killed as a null; do not re-derive.**
* **Adding `"sound"` to an existing check's `observe` list moves the census by
  exactly zero.** `ci/vanilla-reach-census.py`'s numerator is measured off
  recordings, and a recording holds every observation the recorder saw rather
  than only the declared ones — the 29 already came from the whole corpus.
  Only a scenario that makes vanilla *emit* a new event moves it.
* **The seed decoder is still correct**, checked rather than assumed:
  `tools/oracle-runner/src/trace.rs` lines 1204-1256 read and drop both the
  `long seed` and the pitch, and keep the sound id, the source, the position
  and the volume. No second random field has appeared.
* **`ci/t2-gameplay-subset.sh` and `ci/subset-stability.sh` are the two
  instruments for this shape of work** — verdicts in a minute, a floor over N
  cold vanillas in minutes. Neither existed when most of this corpus was
  written and both are how the 92 checks' worth of iteration cost was avoided.

**WHAT WAS BUILT.**

Six scenarios, `730`-`735`, eight block placements each, 48 sound events the
corpus had never heard. Every one is the **sound-beside-the-state-change**
shape: `BlockItem.place` offset 240 passes the placer (offset 210) to
`Level.playSound` as the excluded entity, so the sound is on the observer's
stream and the `block_update`/`block_changed_ack` on the actor's, and the
files set no `observe_from` so the default reads both. The blocks are chosen
so a table that guessed the event from the block's name would be caught:
`dirt` is `block.gravel.place`, `rooted_dirt` has a row of its own,
`nether_gold_ore` and `nether_quartz_ore` are two rows, and cherry, crimson
and bamboo planks are three rows where every older wood shares one.

Each holds its eight items once in `setup` as well as in the step that places
them, so `RecipeBook.add` has already returned false by the time the measured
window opens and no `recipe_book_add` lands in it. `040` observes
`packet:0x4A` because it does not do this.

**WHAT IT MOVED.**

```text
                       before        after       denominator
  sound_event         29  1.47%    77  3.91%     1968
  block               43  3.60%    90  7.53%     1196
  block_state         87  0.27%   134  0.41%     32366
  corpus size         92            98
```

Verdicts `6/6 match` declared, `0/6` strict, `0 vacuous, 0 mute, 0 inert,
0 error`. Vanilla emitted exactly 8 `sound` observations in each of the six —
the reference is live, not silent, which is the check a first-run `match`
has to pass. The two corpus scores are **not comparable**: 92 and 98 are
different denominators, and §20 is the section about what forgetting that
costs.
