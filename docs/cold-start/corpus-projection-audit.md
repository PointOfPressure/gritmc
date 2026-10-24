# `corpus-projection-audit` — what the corpus cannot see, enumerated

Branch carries the rebased `corpus-sound-pitch` work as well; it is one area.
Read `docs/PARITY-ROADMAP.md` §60 (pitch) and §51 (this audit) for the
reasoning. This file is the resumable state.

## The question, and why it had never been asked

Three findings this month were one defect: **a field both servers emit, which
the projection removes before the comparison, so no check can fail on it.**
`sound.pitch` decided claim 420 and was thrown away; 607's greens before
13 Aug were taken through a decoder that could not read an item stack;
`damage_event` left two unnormalised entity ids in a digest. All three were
found by accident. Nobody had enumerated the class.

## The distinction everything turns on

`trace::observe` digests **the bytes the decoder did not consume** into `rest`
and compares that exactly.

* a field **left unread** stays in `rest` and is compared byte for byte —
  opaque, but live;
* a field **read and not emitted** is gone. Advancing the reader is precisely
  what removes bytes from `rest`.

`set_entity_motion` reads its velocity from a *copy* of the cursor so the bytes
stay in `rest`. `sound` reads its seed from the real one so they do not. The
two lines look identical.

## The answer

| | |
|---|---|
| fields the projection removes | **16** — 10 discards, 6 normalisations |
| of those, `decides` a live claim | **2** |
| `armed` (real behaviour, uncomparable) | **8** |
| `note` (no behaviour: random ids, wall clocks) | **6** |
| packet kinds never recorded at all | **12** — 2 vacant, 2 clock, **2 recoverable**, 6 chunk-send |
| kinds `decode` can emit | **37** |
| checks discarding a whole recorded client | **102 of 115** |

The two that decide:

* **`informative.block_update_allowance`.** Its subtraction is *right* — a
  `place` step puts its block down whatever the server believes — but after it
  420 had one observation left, a sound, whose pitch was dropped by a different
  line in a different file. Neither line is wrong alone.
* **`project.observe_from`.** 102 of 115 checks discard a whole recorded
  client, and reading the discarded half found 551's fifth step sound. The
  denominator moved with the entity family and the numerator did not: all seven
  checks it added keep both clients, and **11 of the 13 checks that keep both
  are entity checks**.

Run `ci/projection-drops.py --rank` for the current tally; the ledger is
re-derived against the check files on every run, so these numbers are the
tree's, not this file's.

## What is in the tree

* `ci/projection-drops.py` — enumerates the sites, refuses an undeclared one.
  Eleven refusals, all falsified, thirteen cases. In T0. `stage` is compared
  against the site as of 2026-08-19: it says whether a drop needs the recorder
  rebuilt or only a projection edited, and it was the one column nothing
  checked.
* `ci/checks/projection-drops.toml` — the ledger. `impact` is the column it
  exists for; `observed_by`/`observed_by_count` are re-derived every run so
  the ranking cannot be a memory.
* `ci/projection-drops-falsify.sh` — 9 cases, in T0 beside the check, because
  it mutates copies in a `mktemp -d` and never writes to the tree.
* `ci/hidden-stream-audit.py` — reads the client `observe_from` discards. Not
  gateable: it needs recordings, and T0 is tree-only.

## Every discard site must now be marked

    // DROP: <name> — <why>

and named in the ledger. An unmarked discard is a T0 red. Normalisation rows
anchor on a **function** instead, because the loss is the whole of what the
function does.

## Live findings

* **551's step sounds — and the correction is the finding.** One pair said
  vanilla four, ours five. Over every recording on the box: vanilla **4 in 32,
  5 in 39** (n=71); ours **5 in 66, 4 in 0**. Vanilla is a coin flip and we are
  pinned, so it is not "one too many" but **deterministic where vanilla is
  not** — invisible to a per-run comparison, which would flap on vanilla's own
  variance and be declared unstable. `ci/determinism-gap.py` is the instrument
  for the class; it reports 34 `(scenario, kind)` pairs, mostly the two kinds
  already exempted as unstable. The two on `block_update` looked like the
  interesting ones and **are not** — vanilla's extra updates are
  `minecraft:air` at `at_ms = 0` and ~1490 ms, i.e. neighbouring-scenario
  leakage, which the tool does not filter because it counts raw observations.
  **Footstep parity is still unmeasured by the corpus**, because 551 keeps the
  actor and a player is not sent their own step sounds.
* **`pitch.rs` was declared in neither `PROJECTION_SOURCES` nor
  `RECORDER_SOURCES`.** Fixed. `proj:` moved twice on this branch —
  `7cefde0fb56d9f3e -> 9da3f01f03abf9b9` for `pitch.rs`, then
  `-> ffc7d827de64538d` when `measured_reference_kinds` landed in `compare.rs`,
  which is also a projection source. **The final tip declares
  `proj:ffc7d827de64538d`** and `ci/checks/gameplay-floor.toml` still declares
  the first, so it reads STALE PROJECTION.
  **It must not be re-stamped from the recordings on disk** — they predate the
  pitch decoder, carry no `pitch` field, and the new comparator is inert over
  them. That is `ci/trace_vocabulary.py` §15.4: one projection id, two
  instruments. The evidence run *prints* a floor and does not `update`, so no
  false stamp is written.
* **`ci/check-sound-pitch-table.sh` was Python.** T0 `bash -n`s every `ci/*.sh`
  and was red on this branch. Renamed `.py`.
* **`protocol_ids.json` joined `client::Tables::load` and not the
  reference-cache key.** `pitch.rs` resolves a sound event id through the
  registry. Left alone, a change to the sound registry would move what the
  recorder can classify while the cache served a reference recorded before it.
  Caught by `reference-cache-falsify`'s `tables-set` — **which was itself half
  a check**: it compared a set derived from the loader against a set somebody
  had typed into the falsifier. Both sides are derived now.
  Two fixtures needed the fourth table too, and the integration one failed as
  `vanilla died before it finished booting on :26462` — four failures naming a
  server that was never the problem. `tables=MISSING` is a silent key
  component: every other absent input refuses by name.
* **`measured_reference_kinds` is now in every report.** The count said how
  much evidence a check has; it never said what of. 550's `1` could not be
  distinguished from a damage event.
* **Four checks report `match` on zero informative observations** — 561, 571,
  581, 608. All four are "changes nothing"/"is ignored" claims, so an empty
  measured step *is* the claim, and all four are tier 2 in
  `gameplay-falsifiable.toml`. Not defects; recorded so the next reader does
  not have to re-derive it.

## The tension worth carrying forward

Three screens here produced hits and hits from all three dissolved on a second
look. **The projection's filters are load-bearing**: reading recordings
without them manufactures divergences as readily as applying them hides real
ones. Every tool added prints and none gates, and each says in its header that
confirming a row means re-reading it with the projection applied.

## Not findings, checked and discarded

* `490`'s 10-against-9 `block_event`: a chest re-announcing itself about every
  250 ms sampled by a fixed window. Both servers repeat at the same rate — a
  free-running-timer coin flip, the same mechanism as `move_entity_heartbeat`.
* `605`/`606`/`500`/`530` hidden-stream hits: the item spawn yaw
  `compare::project` exempts by name.

## Still ungradeable, named rather than left to be rediscovered

* `block.note_block.*` pitch — `Math.pow(2, (note-12)/12)`, equal to 1 ulp.
  Claim 430's sound stays undecided on pitch.
* Baby mob voices — the drawn range is scaled by an age factor the packet does
  not carry.
* A constant emitted inside a declared range — indistinguishable on the wire.
* `add_entity.movement` — real behaviour, compared by nothing. Needs
  server-side player physics.
* The discarded client, for 102 of 108 checks. Readable, not gateable.

## Outstanding

`ci/run-pitch-evidence.sh` takes the one corpus recording this area needs and
chains everything after it: the pitch coverage, the lever/comparator pitches
side by side, a `corpus-falsifiable.py` re-sweep (which makes `perturb:pitch` a
live mutation for the first time), the floor re-measured (printed, never
stamped), the hidden-stream audit on a fresh pair, and the six verdicts
this branch can move. Log: `~/grit-logs-pitch-evidence.log`.

`ci/run-projection-verify.sh` waits for that to finish, ships the **final**
tip — four defects were fixed after the evidence run rsynced its snapshot —
re-runs T0 remotely against it, and pushes `corpus-projection-audit` and
`corpus-sound-pitch` to the same commit. Log:
`~/grit-logs-projection-verify.log`.

**420 is the one to watch.** With the pitch classified, its single informative
observation can finally distinguish subtract from compare. If its green turns
red, the green was never evidence — report that as the success it is.

## What 2026-08-19 added (branch `proj2-drops-ledger`)

The audit branch and `entity-parity` were each right about their own tree and
their union was wrong in five places. See `docs/PARITY-ROADMAP.md` §93.

* **`add_entity.movement` is a `project` drop, not a `decode` one** — it moved
  in `a60e7b78` and the row did not follow. `stage` is now compared against the
  site, and `misfiled-stage` reconstructs that exact pair.
* **Four observer ranks re-derived** against the 115-check corpus: `add_entity`
  7 -> 11, `sound` 6 -> 10, `entity_position` 2 -> 3, `block_update` 74 -> 80.
* **`ci/never-recorded.py` + `ci/checks/never-recorded.toml`** rank the twelve
  kinds `is_housekeeping` refuses, read from the pinned jar. `PING` and
  `TAB_LIST` are constructed by **no class in vanilla 26.2** — the refusal costs
  nothing. `KEEP_ALIVE` and `SET_TIME` are clocks. `PLAYER_INFO_UPDATE` and
  `UPDATE_ADVANCEMENTS` are reproducible server behaviour no check can reach,
  and are the only two of the twelve that cost anything.

**Still open:** `minecraft:player`'s `add_entity.movement`, the untested half of
the narrowing `f91984bb` shipped. `ci/run-proj2-evidence.sh` measures it.
