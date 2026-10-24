# Cold start — branch `chunk-persistence`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `chunk-persistence` off `901b2e0`, 2026-08-05

**A player's build now survives a restart, and the world we write is one
vanilla can open.** Complete and T0 green, not merged — the orchestrator
merges. Remote dir `~/rustmc-chain-persist` on the **Threadripper**. Logs on
the Threadripper: `~/rustmc-logs/t2-vanilla-loads-ours-run{1,2,3}.log`,
`~/rustmc-logs/t2-vanilla-loads-ours-FALSIFY.log`,
`~/rustmc-logs/t2-regiondiff-persist.log`.

#### The briefed premise was half wrong, and the wrong half was the load-bearing one

Briefed from a 16-repo survey as "chunk/NBT support is read-only, writer is
wave-1". **The writer has existed all along** — `anvil::write_region`,
`anvil::encode_chunk`, `nbt::write_named_root`, ~360 lines with its own tests.
The survey was reading the two module headers, which both still said the
writer was wave-1 work. Both corrected.

**The real gap was one level up: `crates/rustmc-server` never linked
`rustmc_world::anvil` at all.** No save path, no load path, no world
directory, no `level.dat` concept, no signal handling, and `Tiles` leaks every
`World` so no destructor could ever run. A placed block went into the in-memory
overlay and was never on a path to disk.

#### What is measured and settled

* **Vanilla can read what we write, and that had never been checked.** Every
  test that ever exercised the writer used *our own reader*, including
  `real_vanilla_chunks_survive_a_rewrite`, whose doc comment says a round trip
  through our reader "is weaker than it sounds" and then does exactly that.
  `ci/t2-vanilla-loads-ours.sh` boots a real vanilla server on a region we
  wrote and reads the verdict out of vanilla's console. **GREEN**, three arms,
  50 chunks preserved across two merging writes, zero region-file complaints
  in vanilla's log.
* **`write_region` would have deleted the world on the first save.** It lays
  out only the chunks it is handed and truncates the region's other 1,023
  slots — correct for the worldgen oracle, catastrophic for a save path.
  `write_region_merging` carries untouched chunks across as raw compressed
  records, so they keep block entities, tick queues and structure references
  that a round trip through our reduced `ChunkColumn` silently drops.
* **`DATA_VERSION_26_2` was 4325 and vanilla writes 4903**, and
  `worldgen-dump`'s decorated path passed a hardcoded `4000` — three values,
  none of them right. Two independent sources agree on 4903:
  `SharedConstants.WORLD_VERSION` from the jar, and the `DataVersion` field of
  a chunk vanilla wrote, decoded without our reader. Pinned against the
  fixture rather than a literal, so a version bump cannot update one copy and
  not the other.
* **Saved heightmaps are recomputed for touched columns.** Vanilla trusts a
  saved heightmap rather than recomputing it, so a stale one is not cosmetic —
  mobs spawn on the ground inside a reloaded tower. `SAVED_HEIGHTMAPS` is four
  maps and deliberately not `CLIENT_HEIGHTMAPS`, which is three: `OCEAN_FLOOR`
  is stored and never transmitted.

#### Everything red before it was green, on the box

```text
  the_data_version_matches_the_one_vanilla_writes
      with 4325                   FAILED   "...would be run through DataFixerUpper"
      with 4903                   ok
  save_round_trip, overlay not applied in save_to
                                  FAILED   "the block a player placed did not survive"
                                           (and the untouched-world test stays
                                            green — the degenerate case the
                                            pairing exists to catch)
      restored                    ok, 2 passed, 32.9 s
  t2-vanilla-loads-ours, poke using the TRUNCATING write_region
      poked (1032,200,1032) = diamond_block in chunk (64, 64)
      anvil-probe: chunk (65, 65) is not present in .../r.2.2.mca
      T2-VANILLA-LOADS-OURS RED — second poke failed
```

**BEFORE RE-MEASURING ANYTHING IN `~/rustmc-chain-persist`, `find crates tools
plugins -name '*.rs' -exec touch {} +` FIRST.** Each falsification above edits
a source, builds, and then puts the original back — and restoring a file gives
it an *older* mtime than the falsified artifact, so `cargo` calls the binary
fresh and silently reuses it. That cost two confusing runs here: a green gate
looked permanently red, and the DataVersion pin failed against a source that
said 4903. Both were the stale artifact, both cleared on `touch`. The final T0
above was taken after a forced rebuild. `ci/handoff.sh` is unaffected — it
gates in `~/rustmc-gate`, which never saw these edits.

**That last one refuted its own prediction and the script now says so.** The
gate's comment claimed arm A was what catches a truncating writer. It is not:
the *first* poke already empties the region, so the second poke cannot find
its chunk and the run dies before arm A, before the chunk-count assertion and
before vanilla boots at all. Red for the right reason, one stage earlier than
predicted. Arm A stands as the backstop for the subtler case — a merge that
drops *some* chunks — rather than as the sole detector of wholesale
truncation.

#### FINDING — the seventeenth check that could not fail, and it is NOT this branch's

**CLOSED 2026-08-05 by `gate-skips-and-scenario-verbs`** — see that branch's
cold-start block above for the fix, the four falsification arms, and the census
of which other skip sites actually fire. The account below is left standing
because it is the correct description of the defect and of why the grep could
never have seen it.

`real_vanilla_chunks_survive_a_rewrite` — the only test this project had that
claimed to check the writer against real vanilla output — **has been skipping
in every gate on the Threadripper for its whole life.**
`oracle-work/t2-regiondiff/` did not exist anywhere on that box.

**`ci/handoff.sh`'s `grep -c "skipping: no vanilla"` guard cannot see it, and
the guard is narrower than its own comment claims.** `cargo` captures a
*passing* test's stderr, so a skip printed from inside a green cargo test never
reaches that grep. The guard catches skips printed by *shell* stages only —
handoff's comment already names `ci/dump-flat-preset.sh` as "the one live
emitter reachable from here", which is accurate, but the surrounding prose
reads as though T0's test skips are covered. They are not.

Measured: `./ci/t0.sh | grep -c "skipping: no vanilla"` = 0 on a tree where
the test demonstrably skips (`cargo test ... -- --nocapture` prints it).

The fixture now exists — `ci/t2-regiondiff.sh`, **T2-REGIONDIFF GREEN,
EXACTNESS 100.000000%**, 9 chunks, `a=82 b=82` — and both that test and the
new DataVersion pin really run. **It exists in `~/rustmc-chain-persist` only.**
The gate directory `~/rustmc-gate` still has no `t2-regiondiff`, so the two
tests will skip there unless handoff's stage 1 links or generates it.

#### What is NOT saved, named rather than discovered later

Block entities, entities and tick queues are written **empty** — the canvas
never held them, so a chest survives a restart as a chest and not as its
contents. `structures` is written empty too. None of this stops vanilla
loading the world; all of it is real data loss for a player.

#### The single next item, and what closes it

**Prove a column built from OUR canvas is vanilla-loadable, not just a column
we re-encoded from vanilla's own.** The gate as written pokes blocks into
chunks *vanilla generated*, so `read_region` supplied the sections, palettes
and heightmaps and our encoder only round-tripped them. `World::save_to` and
`worldgen-dump full` instead build the column from scratch via
`column_from_canvas` — our block names, our `pack_heightmap` output, our
`minecraft:full` status — and **vanilla has never been shown one**. Both paths
share `encode_chunk`, so the framing is untested, not the framing code.

Cheapest route: `worldgen-dump full` into a copy of the fixture world's region
directory (it writes the same encoder path `save_to` does), then the existing
stage 4 of `ci/t2-vanilla-loads-ours.sh` with the expected block read back out
of the file. Closing condition: vanilla confirms a block in a chunk our
generator wrote, **and** logs zero region-file complaints.

Second item, smaller: a clean stop currently loses up to `--autosave-seconds`.
A SIGTERM hook beside the timer would fix that; it needs a dependency, and
Cargo.toml says dependency additions go through the orchestrator.
