# Cold start — branch `gameplay-sound-eventid`

---

### COLD START — branch `gameplay-sound-eventid` off `main` (`208e34ef`), 2026-08-17

**The extract's `sound` column carries an event id now, and the thing that
was actually blocking block sounds turned out to be the pitch, not the id.**
Worktree `/home/sayer/grit/work-sndev`. Not merged — the orchestrator gates.

#### The two roadmap rows, re-verdicted before anything was built

Both were stale, in opposite directions, and checking cost one `report.json`
read rather than a jar read or a recording.

* **`block sounds, deterministic pitch` — still open, every specific claim in
  it wrong.** The emitter has had **four** arms since 2026-08-11 (lever,
  comparator, fence gate, button), not the two the row named, and every line
  reference in it was stale. **420 is CLOSED** — `match` on both projections,
  `missing_kinds: []`, in the 13:57 recording — so "420's entire divergence is
  this one packet" no longer holds. 430's first divergence is no longer the
  `basedrum`/`harp` instrument either; it now reports
  `['block_event', 'sound']`.
* **`block sounds, random pitch` — not a sound row at all.** 380 reports
  `['block_event']`, 390 `['block_event', 'section_blocks_update']`. Neither
  names `sound`. Both are behind the unported piston.

#### What was real, and what it cost to fix

`UseGolden`'s two `playSeededSound` overrides were handed the event, the
category, the volume and the pitch, and set `sounded = true`. They now record
`soundEvent`, `soundSource`, `soundVolume`, `soundCount`,
`soundExcludesActor` and `soundPitchDrawn`, and `world_send::use_sound` has a
fifth arm that reads them **behind** the four transcribed families, which
each know something the table does not.

#### The finding that matters more than the feature

**An event id closes only the states whose pitch is a constant.** 1,509 of
4,453 sounding states, 34 distinct events. The other **2,944 sound with a
pitch vanilla draws** — every door, trapdoor and fence gate — and no amount
of re-extracting will move them, because the extract deliberately refuses to
publish a pitch its own fixed seed produced. The roadmap row priced this item
as "the column is a boolean, give it an id, the arms become a table", and
that is half of it. (What closes the other half is a single shared formula
rather than 2,944 ports — see below. That was not obvious until the families
were counted.)

#### The near-miss, written down because it nearly shipped

`soundPitchDrawn` first shipped **inferred** from `usedRandom` — "did the rule
touch the RNG". That is precisely the detector `TableLevel.harnessRandom`'s
own doc comment records as tried and wrong, naming the four blocks it
misclassified last time. It did it again, and under that reading coverage was
**208** — *exactly* the four families already hand-ported. A whole extraction
change would have added nothing and looked like it worked. Measured across
the eight `SEEDS` instead, the same file yields 1,509. **208 is now the
registered floor beside that figure**, and it is a floor in the sense rule 6
wants: what this identical table yields when the instrument is wrong.

Second near-miss, same commit: `soundPitchDrawn` is a property of a *sweep*
and I put it in `sameRow`, which compares *probes*. Every drawn-pitch block
came back probe-dependent on all eleven axes — doors depending on
`full_inventory` and `game_master` — because the stance probes carry the
default `false`. It belongs with `random` and `faceSensitive`, which are not
in `sameRow` either.

#### And the remainder was one formula, not 2,944 problems

Every one of the 2,944 is a door (1,280), a trapdoor (1,280) or a fence gate
(384) — nothing else, all volume 1.0, all excluding the actor, all drawing
`nextFloat()*0.1+0.9`. That is the reading the fence gate was hand-ported on
in 2026-08-11, before the table could name events. So `use_sound` emits them
with the table's event and the shared formula's pitch, and

    states that emit a use-path sound   592 -> 4,453 of 4,453

**This is not parity on the pitch and must not be quoted as it.** The draw is
ours; it cannot equal vanilla's, and no check grades it. It is a real packet
where there was silence. `ci/sound-coverage.py` refuses a **fourth** family,
because one would reach neither arm and be silently silent.

#### Nulls — do not re-run these

* **Do not try to close 380/390 with a sound.** Neither names `sound` in its
  missing kinds; they are piston work.
* **Do not re-open 420 as a sound divergence.** It matches.
* **Do not record the drawn pitch into the table to raise coverage.** The
  harness fixes the seed, so the value is a constant of
  `RandomSource.create(0L)` and publishing it would be a number measuring its
  own input.
* **Do not pair the coverage figure with `use-table-probe-reads-20260811`.**
  That one counts rows (184), this counts sounding states (4,453).
* **Do not port doors, trapdoors or fence gates one class at a time.** They
  are one formula and are already done; the events come from the table.

#### What no check can grade, and it is the important half

**`oracle-runner` deliberately drops `sound.pitch`** (`trace.rs`, `let
_pitch`, with a unit test asserting the pitch is not part of the
observation). So every pitch claim in this area is invisible to the corpus
**in both directions**, and a green on one is not evidence.

The sharp instance is not hypothetical. `sounds::lever_click` differs between
a lever going on and a lever going off **only in its pitch** — same event,
same source, same position, same volume (0.3, 0.6 against 0.5). So
`lever-flips-when-right-clicked` reporting `match` **cannot distinguish a
lever that turned on from one that turned off**. The same applies to the
comparator's 0.55/0.5 that scenario 420 is named for.

#### The single next item

**Teach the recorder to keep `sound.pitch`, behind its own column so the seed
stays dropped.** Until that exists, neither block-sound row can be closed on
evidence, only on transcription — and the row literally named "random pitch"
cannot be graded at all. The closing result is a recording in which
`lever-flips-when-right-clicked` can be made to fail by swapping the two
constants in `lever_click`.
