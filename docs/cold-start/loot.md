### COLD START — branch `loot`, off `main` (`d48904cb5`), 2026-10-01

**The loot-table engine: random sequences, `LootTable.fill`, the entity and
enchanting vocabulary, and the two uses this server can reach — a chest a
creative player places with a loot table, and a chest generation placed.**
Worktree `~/grit/work-loot` (detached), remote dirs `~/rustmc-loot` (recordings)
and `~/rustmc-loot-t0` (T0). Not merged — the orchestrator gates.

**MEASURED (Threadripper, all against vectors vanilla produced).**

* `cargo test --release -p rustmc-loot --test golden_loot -- --nocapture`,
  fixtures from `ci/loot-golden.sh`, which now loads the WHOLE datapack
  through `WorldLoader.load` (tags bound in every registry, nested tables
  resolved, a stand-in `ServerLevel` carrying only `registryAccess`):
  * 18,055 cases: 17,993 MATCH, 0 MISMATCH, 62 REFUSED, 0 ORACLE-BLIND
    (was 18,003 / 17,943 / 0 / 60 / 12 excluded — the twelve that "all
    diverged" were the old oracle rolling a nested table as empty);
  * random sequences: 12 rows (3 world seeds x 4 ids x 8 `nextLong`) exact;
  * chest fills (roll + splitter + `getAvailableSlots` + `shuffleAndSplitItems`
    on one generator): 212 match, 0 mismatch, both generator arms;
  * entity tables in the no-entity context, legacy seeds and the table's own
    random sequence twice from one store: 654 match, 0 mismatch, 0 refused.
* `cargo test --release -p rustmc-server --lib -- lootroll::`:
  enchanting chest tables through this server's `select_enchantment` — 99
  cases and 121 fills match, 0 refused; a pig's two consecutive deaths on the
  `entities/pig` sequence equal vanilla's rolls; seed 12345's four generated
  dungeon chests (chunks 54..62 x 74..82) carry vanilla's own
  `LootTableSeed`, pinned from five vanilla harvests that agree to the bit.
* Falsified, each by name: drawing `nextBoolean` for a count-1 piece, an
  ascending `Util.shuffle`, popping slots from the front, dropping either half
  of a sequence seed, skipping the unenchanted chance draw, answering
  `killed_by_player` true, drawing an enchantment level for a one-level
  enchantment — every one turns its gate red.

**ON THE WIRE.** `ci/checks/gameplay/850`-`853`, recorded with
`ci/t2-gameplay-subset.sh` (090 as the chest control), GAMEPLAY_SEED 20260803:
**declared 5/5, strict 5/5 match**. 850 seeded weaponsmith (9 stacks), 851
seedless weaponsmith (12, first draw on its sequence), 852 the same table again
(12 different stacks — the sequence advanced, not re-created), 853 seedless
`simple_dungeon` (17). Vanilla's slots equal ours and equal `LootGolden`'s JVM
fills for the same arms. All four tier 2 in `gameplay-falsifiable.toml`
(swept over the gate's pair with these four spliced in).

**PERSISTENCE.** `random_sequences.dat` now carries the sequences (it was
written empty), and `--world` restores them. Seeding checked against a file
vanilla wrote: `oracle-work/gen-20260722-25852`'s five block-drop sequences are
this crate's streams after 0, 0, 0, 27, 0 draws.

**KILLED AS NULLS — do not re-run.**

* "Use the `damage`/`kill` console verbs on main" — they are not on main
  (`console.rs` understands `gamerule`, `weather`, `worldborder`,
  `scoreboard`), and the branches that have them kill a PLAYER by name. No
  path on main kills a mob, so mob drops are not gradeable on the wire from
  this branch. The API a death path calls is `crate::lootroll::mob_drops`.
* `container_loot` cannot be sent by a client (no stream codec). Do not try;
  `block_entity_data` is the route.
* A JVM golden for entity tables "with an entity" — there is no level to make
  one in. The no-entity context is exact and is what the vectors use.

**OVERLAP, named.** `entity-ports` / `ranged` carry their own entity loot
(8a267c798: `EntityCtx` with equipment and damage tags) and a
`crate::loot` + `crate::randomsequences` whose store is never seeded on a live
server. This branch adopted their exact generic `roll_with` signatures so
those hunks are identical, and named its server module `lootroll` to avoid an
add/add. Whichever lands second folds the duplicate entity conditions and the
two sequence stores; this branch's are gated against vanilla vectors.

**NEXT.** Wire `mob_drops` (or the merged equivalent) into the first mob death
path that lands, and record a kill: the pig's porkchops come off
`minecraft:entities/pig` at world seed `GAMEPLAY_SEED`. Closes when that
scenario reaches tier 2.

**STATE at halt (2026-10-01).** Tip after this note; T0 GREEN at 3b6b01b25 (`~/rustmc-loot-t0`). 850-853 recorded once (5/5 match); the second recording for self-agreement (`loot-subset2`, detached) was still queued on the corpus lock — its output lands in the scratch `runs/r2` and has not been compared.
Next: compare r2 against r1, then wire `lootroll::mob_drops` into the first mob death path that lands.

### COLD START — branch `loot`, repeat evidence, 2026-10-01

`loot-subset2` finished (rc=0): 850-853 + 090 declared 5/5, strict 5/5 again.
Vanilla r1 vs r2 floor 0/5 (N=2); crossed vanilla/rustmc pairs 5/5 strict both
ways. The loot rolls on the wire are deterministic run to run on both sides.
`git rebase origin/main` (03fda6cd0..+49) is clean, no conflicts.
Next is unchanged: mob drops on the wire once a mob can die.
