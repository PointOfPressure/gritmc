# COLD START — branch `light-engine-2`

### COLD START — branch `light-engine-2`, 2026-09-05

**The area: turning the ported light engine on.** Built on `light-engine`
(itself `stack-worldgen` + 14), which is main + 390 + the engine. This branch
is the four things that stood between an exact engine and a client that can
see it: the default, live edits, the `light_update` packet, and the End's
residual.

### The headline

    RUSTMC_SECTION_LIGHT                  defaults ON; `=0` restores the old wire
    the chunk-packet golden under it      117 of 117, and it CANNOT see the flip
    our light vs vanilla's packet BYTES   245 of 2,572,288 nibbles (settled capture)
    the live edit path                    0 of 1,916,928 nibbles vs the graded instrument
    light_update on the wire              12 pairs named, 12 pairs moved, filters clear
    the End's residual                    still 5,609 of 1,863,680 — and now attributed
                                          exactly: 32,768 of 32,768

Nothing here is a percentage, for the reason the previous cold start gives:
saved light has no innocent-vanilla floor, so counts are the primary form.

### 1. The default is ON, and the check the orchestrator named could not grade it

`docs/PARITY-ROADMAP.md` §75 named the reading that closes the flip and the
`light-engine` lane took it: `light_harvest`, three dimensions, 0 / 0 / 5,609.
The flip is `7cec64323`, with those figures in the message.

**The chunk-packet golden did not move and could not have.**
`tests/golden_chunk_packet.rs` decodes vanilla's captured bytes and re-encodes
them; it constructs no `World`, reads no flag, and is green whether our light
is exact or absent. Re-taken today with `ci/dump-chunks.sh` it is still 117 of
117 and 5,616 containers. **That green is not evidence about the section
light and never was.**

What grades the flip is new: `tests/golden_chunk_light.rs` lights **vanilla's
own blocks, decoded out of the same 117 packets**, and compares against the
light vanilla shipped in those same bytes. Our generator is not consulted, so
no worldgen residual can reach the number.

    chunk-packet light, vanilla first: 245 of 2,572,288 over 77 scored chunks
    control: the capture's commonest non-air state emitting 15 moves 1,660,883

### 2. VANILLA'S CHUNK PACKET LIGHT LAGS ITS OWN BLOCKS

The most useful thing this branch found, and it is about vanilla, not us. Two
dumps from **one** vanilla server 120 s apart, same seed, same chunks, graded
by the same deterministic engine:

    capture       block nibbles   sky nibbles   window
    first          5,261            190         2,437,120
    +120 s            55            190         2,572,288

`RUSTMC_LIGHT_DUMP=1` attributes it: the first sample sits 14 blocks from
`minecraft:lava[level=2]` whose *source* block is one block further out. Fluid
ticks spread the lava after the chunk was lit, the repair went on the light
thread, and the chunk was serialised with the new block and the old light.
**Ours was one brighter — exactly one lava block's worth — and ours was
right.** The correction vanilla sends is a `light_update`.

`ci/dump-chunks.sh` takes a settle time now. **A capture taken the instant the
server boots grades the fluid ticks, not the light.**

A third capture, from a different run of the same seed, carries 299 sky
nibbles and 27 absent sections where the other two carry 190 and 26 — vanilla's
own worldgen non-determinism reaching a light comparison. That is why
`golden_chunk_light` asserts a gross 0.1% ceiling and the one invariant no lag
can explain (we never set a mask bit vanilla left unset), and reports the rest.

### 3. The light is live

`World::light` was a `OnceLock`. `LiveLight` replaces it and each method is
named for the vanilla method it is: `LevelChunk.setBlockState`,
`ThreadedLevelLightEngine`'s drain, `swapSectionMap`/`onLightUpdate`,
`ChunkHolder.sectionLightChanged`, `ChunkHolder.broadcastChanges`.

    post-edit live path: 0 of 1,916,928 nibbles differ from
    light_region_with_edits over 9 chunks, 3 edits, seed 20260722

The oracle is `light_region_with_edits` rather than a hand-written
expectation, because that function is the one graded at 2,340 of 2,340
post-edit sections against vanilla's own engine. Controls: a withheld edit
moves the figure by 2,342; a placed torch's own nibble must be its emission of
14.

Two departures, both stated in the code: the drain runs at the next *read* of
the light rather than on a light thread (same converged answer, same one
repair per batch), and a section that has just become all-air is not
undeclared, because `LevelChunk.setBlockState` does not undeclare either.

### 4. `light_update` on the wire

Ported from `ClientboundLightUpdatePacket` — varint x, varint z, then the
chunk packet's light tail byte for byte — and sent from the server tick
**before** `changed.flush`, which is where `ChunkHolder.broadcastChanges` puts
it (offsets 57-86 vs 111). A client given them the other way round renders one
frame of a lit torch in an unlit room.

    light_update after one torch: 6 chunks, 12 section/layer pairs named,
    12 pairs changed while present on both sides, 18 that started or stopped
    being stored

**The 18 are the load-bearing exclusion and they are vanilla's.** A block
landing in a previously all-air section makes a whole 3x3 of chunks start
storing that section (`updateSectionStatus` walks a 3x3x3 cube);
`markNewInconsistencies` adds those to **`changedSections`**, not to
`sectionsAffectedByLightUpdates`, and only the affected set reaches the
packet. The first version of the check demanded the mask cover them and was
demanding a defect.

`tools/oracle-runner/src/trace.rs` no longer drops `cb::LIGHT_UPDATE` and has
a decoder arm for it.

### 5. The End's 5,609, attributed to the nibble

    repeatFirstLayer probe, section -1: 32,768 of 32,768 of vanilla's nibbles
    equal OUR converged value at the bottom slice of the section above

Every one. It is an assertion in `light_harvest`, not a printout, because it
is the thing that could stop being true. Vanilla's padding layer is
`repeatFirstLayer` of a **lit** section; ours is what propagation left in a
layer created before that section had light. A layer created at the wrong
moment, not a light value computed wrongly.

The same rule explains why only 8 of 81 scored chunks save the array at all:
`d.copy()` of a homogeneous zero layer keeps `data == null`, `isEmpty()`
holds, and the serialiser writes nothing.

### Nulls — do not re-run

* **Do not quote `golden_chunk_packet`'s 117 of 117 as evidence about the
  section light.** It is a byte round trip and is green either way.
* **Do not grade light out of an unsettled chunk capture.** Vanilla's own
  packet is internally inconsistent for as long as the fluid ticks run.
* **Do not re-run the per-chunk driver as an order sweep.**
  `light_incremental::light_region_per_chunk` exists and is measured: it takes
  the End from 5,609 to **14,537** while the overworld and nether stay at 0.
  `(x, z)`-ascending is not vanilla's chunk load order and the remaining
  variable is a *load* order a saved region file does not record.
* **Do not write `repeat_first_layer` of the converged slice into the padding
  section at harvest time.** It produces vanilla's exact bytes by a route
  vanilla does not take. Refused on the fidelity rule.
* **Do not report that un-dropping `light_update` made `gameplay-floor.toml`
  stale.** It was already stale on `main`: the file pins
  `proj:7cefde0fb56d9f3e`, main computes `proj:1277e43a7e157183`, this stack's
  base `proj:abf4647324dc8caf`, and this branch `proj:9823c4267825d586`.
* **Do not quote `31 / 129 / 141 = 24.03%`.** Un-dropping a packet moves that
  ceiling — it is `141 - len(is_housekeeping) - len(unbuildable)` and the
  census prints it — and it is **131** now. The numerator cannot be re-taken,
  so `corpus-reach-ceiling-protocol-20260812` is withdrawn as `STALE CEILING,
  RECORDER MOVED`. 31 of 141 = 21.99% of the registry stands.
* **Un-dropping a packet reddens three ledgers, not one.** In order:
  `ci/vanilla-reach-census.py`'s `ceiling_premise_housekeeping`,
  `ci/chunk-send-unrecorded.py`'s STALE-ROW, and `ci/substrate-gate-audit.py`'s
  UNATTRIBUTED. A serial gate meets serial pins one run apart, so grep `ci/`
  for `is_housekeeping|DECODED_KINDS|trace\.rs` — sixteen files — and pre-run
  all of them in a `ci`-only `$ROOT` first. `ci/decoded-kind-reach.py` is the
  one that cannot be pre-run: every recording on the box predates the arm.
* **`commands` is UNATTRIBUTED in `ci/substrate-gate-audit.py` and always
  was** (verified against `origin/light-engine`'s `trace.rs`). That audit is
  in no gate. Not this branch's, and not guessed at.
* **The subset capture did NOT happen.** `ci/t2-gameplay-subset.sh` sat six
  minutes in a corpus queue three deep behind a merge gate and died at
  `SUBSET-EXIT=255` without ever taking the lock, so **no vanilla
  `light_update` bytes were recorded and none are graded**. That is still the
  single next item; the recorder is ready for it now.

### The single next item

**Grade our `light_update` bytes against a vanilla one.** The recorder now
keeps the packet and has a decoder for it, and nothing in the corpus has ever
held one, so the first recording of a torch placement against real vanilla is
the whole of the remaining evidence. It closes when a scenario's
`light_update` observation matches on both sides — chunk, four mask popcounts,
two array counts and `rest`. The run is
`RUSTMC_DUMP_PACKET=light_update ./ci/t2-gameplay-subset.sh
150-torch-on-a-wall-faces-out.toml`, which needs the box-wide corpus lock.

The 190 sky nibbles `golden_chunk_light` charges are **NOT** that item and are
not open: they are vanilla stale against its own rule. Vanilla's saved sky is
15 at an `oak_leaves` block whose `getLightDampening()` is 1, which
`ChunkSkyLightSources.isEdgeOccluded`'s first line makes an occluder, so
vanilla's source table for that column predates the tree; 143 of the 190 sit
on a chunk face against 23.4% of columns, and a tree that straddles a border
is written by the *neighbour's* FEATURES stage, where
`ProtoChunk.setBlockState` runs no `ChunkSkyLightSources.update`. Our
transcription is that bytecode line for line. Do not chase it.
