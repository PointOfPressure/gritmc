# Cold start — branch `redstone`

### COLD START — branch `redstone` off `main` (`bd5be4a36`), 2026-09-05

**Area: the block-update half of `neighborChanged` and every component that
rides on it.** Worktree `~/grit/work-rs`, remote dir `~/rustmc-rs`. Not
merged — the orchestrator gates.

## The brief's premise was stale, and what main actually had

`docs/SERVER.md:156` said "No `neighborChanged`". `crates/rustmc-server/src/
power.rs` (4,103 lines) already carried `CollectingNeighborUpdater` with
`NeighborUpdater.UPDATE_ORDER`, the wire's `DefaultRedstoneWireEvaluator`
(the one 26.2 runs; `REDSTONE_EXPERIMENTS` is off), the torch with its
burnout list, the repeater, the observer, the lamp's on-half and the signal
table; `ticks.rs` (938) a `LevelTicks` with `DRAIN_ORDER`; `piston.rs`
(1,692) `checkIfExtend`, the block event and the non-sticky resolver. What
main lacked, each measured before it was written:

* **`affectNeighborsAfterRemoval` for every component** — nothing ran it.
  `LevelChunk.setBlockState` 440-490. Now `removal.rs`.
* **The lamp's off tick** — `lamp_wants_a_tick` printed instead of
  scheduling; every lit lamp stayed lit for ever (`962` on
  `origin/stack-gameplay` measures it).
* **The comparator** — refused by name. Now `comparator.rs`, with the
  analog reads and its block entity's one `int`.
* **The piston's second half** — `moveBlocks` was a raw write list with no
  block entity, so the head never landed (380/390's first difference) and a
  retracted piston stayed `moving_piston` for ever. Now `pistonmove.rs`.
* **`willTickThisTick` / `hasScheduledTick`** — the queue had neither;
  `DiodeBlock.checkTickOnNeighbor` guards on the first.
* **`RepeaterBlock.updateShape`'s LOCKED arm** — a repeater could not lock.
* **`DiodeBlock.setPlacedBy`'s tick** and **`Level.updateNeighbourForOutputSignal`**.

## Refused, and why

* **Pressure plates.** `BasePressurePlateBlock.entityInside` needs an entity
  standing on the plate. The harness has no move verb (`main` and
  `stack-gameplay` carry the same nine: attack, close_container,
  container_click, dig, drop, sign_update, sneak, swap_offhand, swing, plus
  place/look/hold) and item entities do not tick (`item-entity-tick`, retired
  by measurement), so no scenario can press one. The removal hook carries
  the plate's `affectNeighborsAfterRemoval` because it costs nothing; the
  press itself is not written, because nothing could show it red.
* **The daylight detector's ticker.** `DaylightDetectorBlock.updateSignalStrength`
  reads `Level.getEffectiveSkyBrightness` and
  `EnvironmentAttributes.SUN_ANGLE` — the 26.2 attribute system, layered
  from the datapack (`EnvironmentAttributeLayer$TimeBased`), on top of a
  sky-light engine this server does not have. The right-click half is on
  main and `710` matches; the 20-tick ticker would need both subsystems.
* **Item frames** as a comparator input (no item-frame entity), and the
  analog values held in block entities this server does not model — listed
  in `comparator.rs`'s header, refused once per state on stderr.
* **`moveCollidedEntities`/`moveStuckEntities`** — no scenario stands an
  entity in front of a piston.

## Numbers

`cargo test --release -p rustmc-server --lib`: 679 -> 701 passed, 0 failed,
Threadripper, `~/rustmc-rs`. Clippy `-D warnings` clean at every push.

Nine scenarios added, `455`-`459` and `461`-`464`, all parsing under
`oracle-runner compare` (reported `error — missing from a trace` against
the pre-branch recordings, as a new scenario must). Their tier is decided
by the detached `redstone-t2` chain: `ci/t2-gameplay.sh`, then
`ci/corpus-falsifiable.py --sweep`, then the ledger commit. Until that
lands, `ci/corpus-falsifiable.py --check` reds T0 with nine UNREGISTERED
rows — expected, and the only T0 red this branch introduces.

## Next

The three previously-diverging checks — `380`, `390`, `400` — are expected
to flip with this branch (the head lands two ticks after the event; the
torch's tick was already on main and the lever's second sweep is on
`stack-gameplay`). If `380`/`390` still diverge after the chain, read the
first difference for the **order** of the head's and the stone's
`block_update`s within the tick: they are one `section_blocks_update` in
vanilla, and this server coalesces per section too, so an order difference
there is the ShortSet's, not this branch's.

### COLD START — branch `redstone` rebased onto `1d109abcf`, 2026-09-05

**A landing pass. Nothing about the server moved, and nothing about this
branch's evidence needed re-taking.** 8 commits replayed onto main at
1d109abcf (2,788).

    REBASE     8/8 replayed. ONE textual conflict, `docs/FINDINGS.md`, add/add,
               both sides kept in the file's own order.
               `ci/checks/gameplay-baseline.toml` conflicted and was resolved by
               `ci/merge-docregions.py` — "1 pure-append hunk resolved inside a
               declared append region; 0 left conflicted" — which is what the
               declared merge region is for: main's own nine baseline locks
               (`dd3da1177`) and this branch's nine rows both landed.
               `git range-diff` over the two ranges shows three commits changed
               and all three changes are relocated context, not content.
    GREEN      In the worktree, post-rebase, no build needed:
               `ci/corpus-falsifiable.py --check` **117/117** (0 unswept, 4 dead
               fields, 0 without a localised kill) — the nine UNREGISTERED rows
               the previous entry warned about are registered and stayed so;
               `ci/corpus-grades.py --check` 117 checks, 60 frozen, 9 unresolved
               (N<2) over 28 recordings — 60 is under `HISTORY_FROZEN_CEILING`
               61 and 9 under `HISTORY_UNRESOLVED_CEILING` 14, which is the
               `corpus-instruments` reclassification landing exactly as its own
               cold start predicted for this branch (`69 frozen (RED)` ->
               `60 frozen + 9 unresolved (PASS)`);
               `ci/corpus-machinery-falsify.py` GREEN, 18 assertions;
               `ci/liveness-proofs.py --check` 10/10;
               `ci/mob-observability.py --check`, `ci/check-figures.py`,
               `ci/check-checks-present.py` (15/15) all green.

**Needs hands — a collision between this branch and `mob-in-world`, and it is
the ONE thing landing order decides here.** Both branches rewrite
`[meta]` in `ci/checks/gameplay-falsifiable.toml`, which is **outside** that
file's declared merge region. `mob-in-world` sets `DEAD_FIELDS = ()` and
`FIELD_UNIVERSE = 95` in `ci/corpus-falsifiable.py` (a pig and a bat now stand
in every recording, so the four `move_entity_rot`/`rotate_head` pairs moved to
`undetermined`); this branch's ledger carries `dead_fields = 4` against main's
pinned four. Whichever lands second will red on
`DEAD-FIELDS [meta] says dead_fields = 4 and 0 are pinned` and **cannot be
resolved by hand** — the honest repair is `--sweep` over that branch's own
recording pair, ~95 s, no lock, no server. It is not a defect in either
branch: it is one constant that is a property of the corpus, and the corpus
changed underneath it.

**Killed as a null, do not re-run:** *"the rebase invalidated the nine
scenarios' evidence"* — it did not. `--check` reads the ledger against the
scenarios' semantic digests, and 117/117 carry a localised kill post-rebase.
