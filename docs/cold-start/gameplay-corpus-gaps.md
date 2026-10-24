# Cold start — branch `gameplay-corpus-gaps`

Append-only. A new `### COLD START` entry per turn on this branch; nothing
above is edited.

---

### COLD START — branch `gameplay-corpus-gaps`, off `main` (`b94f3ebf`), 2026-08-18

**The area was every open row §52 left: 430, 400, 070, 780, 790.** Worktree
`~/grit/work-corpusgap`, remote dir `~/rustmc-corpusgap`. Not merged — the
orchestrator gates.

#### The first finding is that two of the five were already done and unpushed

Before writing any code, `git worktree list` on this box shows branches for
four of the five claims. Two carried real, finished, **unpushed** work:

* **070** — `origin/gameplay-motion-combat` (`47702419`), pushed but never
  recorded. Rebased cleanly; it is based on exactly `b94f3ebf`.
* **790** — a local branch confusingly named `gameplay-780-random-tick` in
  `~/grit/work-tickpot`, four commits, whose cold start is
  `gameplay-790-flower-pot.md`. **Never pushed anywhere.** Recovered here as
  its original four commits.

Both are cherry-picked onto this branch unchanged. A third, `work-ticks`
(`gameplay-790-corpus-verdict`), has a **zero-byte commit object** in
`.git/objects/fd/8cf9c7…` — its HEAD is unreadable and nothing in it is
recoverable. It is not needed: the 790 work above is the complete port.

#### Two briefed facts were refuted by the tree, before any measurement

* **"The `block_props.json` instrument column is on `origin/block-events`,
  NOT on main."** `origin/block-events` is an **ancestor of
  `origin/main`** — merged — and neither it nor any of the other 202 remote
  branches nor any local branch has ever had an instrument column. `git grep
  instrument -- tools/goldengen/vanilla/BlockPropsGolden.java` matches
  nothing anywhere. That work did not exist and is written here for the
  first time. Its numbers, though, were exactly right, which is why it read
  as a memory of real work: 27 instruments, 32,366 states, 24,234 non-harp.
* **780 is not open.** It is closed on `origin/gamerules-console`
  (`b8bb2c12`), and a solo recording in `~/rustmc-gamerules` reads
  `match`/`match` 2→2 where the full corpus reads `absent`. Not re-ported;
  attributed in §56 so two agents cannot both port it. **The obvious check
  on that attribution gives the wrong answer**: the recording is stamped
  20:48 and the tree's `randomtick.rs` 20:55, so by mtime the recording
  predates its own subject. `b8bb2c12` changes no source — checks and docs
  only — so the tip's code is the recorded code and the mtime is an rsync
  touch.

#### What was built

**430 is three defects, not the two the audit lists** — §54. The third is
inside a `block_update` both sides emit, so no aggregate over kinds could
show it: vanilla writes `instrument=harp` and this server writes
`instrument=basedrum`, on grass.

1. `BlockPropsGolden` gained an `instrument` column and an `instruments`
   array (the enum, 27 rows, with its three `Type` predicates and sound
   event). `block_props.rs` reads both, **refusing** a missing column, and
   carries `note_block_instrument` — `setInstrument` itself, ported with its
   two traps: `above` is read first, and the `below` branch **inverts**, so
   a head below gives `harp`.
2. `placement.rs` writes it in `refined_with_neighbours`, beside the stair
   shape and the chest type.
3. `containers.rs` — `trigger_event` is now one call returning both what it
   played and what it returns, because vanilla's `triggerEvent` is one
   method that does both. `RECORDS` ordinal 2 at volume 3.0, block centre,
   pitch from NOTE only when the instrument is tunable.
4. `world_send.rs` — `play_note`, queuing the block event after the write,
   with its guard: only when the note block's **own** instrument works above
   or the block above is air.

**400 was not a missing rule** — §55. Every link of its chain already
answered correctly in isolation, and that is now a test so nobody re-ports
`crate::power`. The defect was that `LeverBlock.updateNeighbours` is **two**
`updateNeighborsAt` calls and this server made one; the second is at the
block the lever is attached to, which is the only way its torch is reachable.
`ButtonBlock` is the same five instructions and comes with it.

**A whole class of dead port, found by reading `Tiles`.** Four `Blocks`
methods have default bodies returning `None`, every caller reads `None` as
"do nothing", and `crate::tiles::Tiles` — the production world — delegated
**none** of them. So `level_min_y` was `None` on the real server, which
skips `FallingBlock.tick`'s whole arm: **070's sand would not have fallen**,
behind a complete port, 604 green tests and a green T0. `note_block_instrument`
was the same shape. `ci/check-blocks-delegation.sh` is now in T0 with its own
`--falsify`, which caught a bug in the checker itself on its first run
(`caught 0 of 4`) before it ever guarded anything.

#### Nulls — do not re-run these

* **Do not look for the instrument bug in `placement.json`.** It cannot be
  there in principle: `setInstrument` reads two positions, neither of which
  is the clicked face, and that table has one cell per face.
* **Do not re-port 780.** See above.
* **Do not restart 070 or 790.** Both are recovered here whole.
* **Do not add a note block arm to `world_send::use_sound`.** It would be a
  duplicate on the wire, not an approximation — the reason is a doc comment
  on that function now.
* **Do not read an empty `missing_kinds` as "nothing is missing".** 400's is
  empty while a whole block change is absent.

#### What no check can grade, and this is the part to read before believing a green

* **`sound.pitch` — no check in this corpus can grade one.**
  `tools/oracle-runner/src/trace.rs` reads it and discards it rather than
  folding it into `rest`. So 430's `sound` observation matching says nothing
  whatever about `getPitchFromNote`, which is ported to the letter and is
  ungradeable in either direction. A fix is in flight on
  `origin/corpus-sound-pitch` and names this call as **still** ungradeable
  afterwards, because `Math.pow` and `f64::powf` agree to 1 ulp, not exactly.
* **The note particle is not a packet.** `Level.addParticle`'s body is a
  single `return`; `ServerLevel` does not override it. There is nothing to
  port and nothing to miss.
* **Neither branch of `setInstrument` is exercised by any scenario.** The
  corpus places a note block on flat ground with air above it. Both the
  above-wins and the below-inverts branches are covered by unit tests
  against the real table and by nothing else.
* **`NoteBlock.updateShape` is not ported and cannot be tabled.**
  `update_shape.json` refuses the block outright — `updateShape read 0,1,0,
  not its own neighbour`. Nothing in the corpus changes a block above or
  below a note block.
* **The redstone `Orientation`** both of the lever's calls carry is not
  reconstructed; nothing reads one.
* **790's other 38 pots** are measured by the table and by unit tests, and
  by no recording.
* **`ci/check-blocks-delegation.sh` checks structure, not agreement.** It
  requires `Tiles` to *ask* the owning tile; nothing asserts the answer
  matches.

#### VERIFIED

`cargo test --release -p rustmc-server --lib` — **626 passed, 0 failed, 1
ignored**. `cargo test --release -p rustmc-behavior --lib block_props` — 13
passed, 4 new. **T0 GREEN** on the Threadripper, including the new
`check-blocks-delegation` and its falsify. No check file was added or edited,
so `ci/corpus-falsifiable.py` needed no re-sweep and reads 108/108 tier 2.

#### THE SINGLE NEXT ITEM

**Read the queued `ci/t2-gameplay.sh` recording** and take the verdicts for
430, 400, 070 and 790 off it. `match` on both denominators closes each.
Anything else is the next diagnosis, and the evidence for it is in the
recording rather than in a re-derivation: 430's remaining risk is ordering
(the sound and the block event must land between the third `block_update`
and the ack), 400's is whether the second neighbour update fires inside the
click handler's own tick, and 070's is the landing position.
