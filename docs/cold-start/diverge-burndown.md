# Cold start — branch `diverge-burndown`

---

### COLD START — branch `diverge-burndown` off `606551b54` (train-a, now in main), 2026-09-30

**Corpus declared 146 -> 171 of 179, strict 121 -> 146** — one full
`ci/t2-gameplay.sh` in `~/rustmc-diverge` on `bf754d7a3`'s server, no
REGRESSED, T0 keep-going green (0 reds) on `594ba5305`.

Fixed, each against the check that recorded vanilla doing it and this
server not, each re-recorded with `ci/t2-gameplay-subset.sh`:

    548 547      chunk sender walks ChunkTrackingView (pad 2, Euclidean), not a square
    430          no second note-block sound from the use table
    963          EndRodBlock turns round on a rod pointing the same way
    732          CandleBlock.canBeReplaced / cycle(CANDLES)
    740-756 742 797   ten block-entity menus, the double chest, lectern books, per-tick menu sweep
    890          mob head on a note block's top: PASS to placement + NoteBlock.updateShape
    730 731      WallBlock (crate::wall), first reader of tags_block.json (crate::blocktags)
    775 774 773  minecart, end crystal, item frame from the hand
    082 (partial) checkDespawn only inside entityTickList

**Refused, vanilla unstable against itself** (`ci/corpus-floor.sh`, 5
recordings, 10 pairs): 080 spawn egg (5 distinct), 500 dropping an item (5),
a-dropped-item-goes-into-your-inventory (5), a-player-that-leaves-the-ground
(2 of 6 flapping pairs). 082 and 087 matched on the full run and their rows
stay `diverge`: vanilla's own output moves (082's pig in the settle 2 of 3
runs today; 087 per FINDINGS).

**Next item — what is left diverging is four ports, each its own lane:**
`776` boat (`BoatItem.use` raycast + `AbstractBoat` physics), `777` armor
stand (a non-`Mob` `LivingEntity` tick: health 20 is the whole
`set_entity_data`, `15fd54b4b21bec41`; the tick-1 `update_attributes`
`8c43623da909a601` is `movement_speed` created by `removeFrost`, same as the
bat's), `788`/`789` golems (`CarvedPumpkinBlock.trySpawnGolem` plus two
`Mob` kinds with their goals). Closing any one is its check going `match`.

Nulls: 082's residual is `ChunkMap.isChunkTracked`'s `!isPending`, not
despawn — do not chase it in `mob`. A supervisor parks uncommitted work in
this worktree as a stash and detaches it; commit before any long wait.
