# COLD START — branch `harness`

Append a new dated entry below; never edit an old one.

### COLD START — branch `harness`, 2026-09-05

**The area.** The corpus harness: `ci/t2-gameplay.sh` and
`ci/t2-gameplay-subset.sh`'s substrate, the `light_update` subset capture, and
the merge collisions between the branches landing on the recorder.

**What moved.**

1. **The two servers were on different difficulties and nothing read either
   value back.** Vanilla booted with `difficulty=peaceful` in
   `server.properties`; this server got no difficulty argument at all and ran
   at its own default. `ci/gameplay-difficulty.sh` is the one place either side
   chooses one now, with `ci/gameplay-difficulty-falsify.sh` in T0 (18
   assertions, 1.4 s, no server and no lock; 14/4 red on the unrepaired
   scripts).

   The bytecode is in that file's header and is the reason it matters: at
   `PEACEFUL`, `SpawnEggItem.useOn` offsets 21-33 return `FAIL` before reading
   the clicked position, because `EntityType.canSpawn` offsets 13-31 are
   `isAllowedInPeaceful() || getDifficulty() != PEACEFUL`. A monster scenario
   was graded against vanilla silence.

   **It could not be fixed by passing the flag.** `main.rs` ignores an argv
   token it does not recognise, so `--difficulty peaceful` handed to a tree
   without the flag configures nothing while the launch line claims a symmetry.
   Support is derived from the server source and the one unmatched state —
   a subject that HAS a difficulty and no way to be told one, which is exactly
   where the three mob lanes were — refuses.

2. **`docs/FINDINGS.md` is the merge queue's largest collision**: 588 of 595
   pairs over the 37 branches that touch it, down to 37 under
   `merge=docregions to-eof check=unique-bullet-line`. `ci/checks/test-targets.toml`
   is second at 193 of 630 over 36 branches, and takes the `ci/t0.sh` shape —
   whole-body region, `check=toml-buckets`.

3. **The merge falsify suite counted itself by hand** and was stale by 14
   cases. Both counts are derived now.

**What was killed as a null — do not re-run these.**

* **`tools/oracle-runner/src/trace.rs` does not collide between the three
  landing branches.** `light-engine-2` (10 hunks) and `advancements` (4)
  auto-merge on it; `gate-instruments` does not touch the file at all. A
  421-commit rebase of `light-engine-2` onto main conflicts at commit 287 on
  `ci/checks/test-targets.toml` and never on `trace.rs`. There is no
  three-way trace.rs resolution to write and no `MERGE-REGION` to add there;
  the `.gitattributes` sweep already ranked it at 2 pairs of 1035 and that
  ranking was right.
* **`gate-instruments` does not move the projection id.** Its `compare.rs`
  change is comment and test text, which `projection_body` strips. main and
  `gate-instruments` are both `proj:1277e43a7e157183`.
* **The subset script did not time out.** `SUBSET-EXIT=255` was ssh's own exit
  code — the run was `ssh -n threadripper '...subset...'` and the connection
  went with the caller six minutes into the queue. The script's own exits are
  1, 2 and 4. Nothing in `ci/t2-gameplay-subset.sh` needed repairing; what
  needed repairing was the invocation, which is `ci/run-detached.sh`.

**The projection ids, measured.**

| tree | projection id |
|---|---|
| `main`, `gate-instruments` | `proj:1277e43a7e157183` |
| `advancements` | `proj:2538a57f06c76644` |
| `light-engine-2` | `proj:9823c4267825d586` |
| all three landed | `proj:fc5dab50ab0b3eae` |

Derived with the tree's own `projection_body` strip and `trace::digest`, and
validated against the binary built in `~/rustmc-light2`, which prints
`proj:9823c4267825d586`. **main's `ci/checks/gameplay-floor.toml` pins
`proj:7cefde0fb56d9f3e` and is STALE PROJECTION today**; `advancements` and
`light-engine-2` carry the same stale pin. `gate-instruments`' floor
re-derivation is the repair. **Re-derive the floor once, after the last of the
three lands, not once per landing** — three landings would otherwise cost three
re-derivations for one final value.

**The single next item.** The `light_update` capture, and after it the monster
scenario. `light2-subset` is queued under `ci/run-detached.sh` against
`light-engine-2`'s tip in `~/rustmc-light2`; the closing result is the
match/diverge count of `light_update` events between
`oracle-work/gameplay/subset/vanilla.json` and `rustmc.json`. Note that
`150-torch-on-a-wall-faces-out.toml` does not declare `light_update` in its
`observe` list, so `compare` will say nothing about it — the count comes from
the recordings, which now hold the packet because `light-engine-2` un-dropped
it from `is_housekeeping`.

The monster scenario needs `GAMEPLAY_DIFFICULTY=easy` and a tree with
`--difficulty`, i.e. one of the mob branches, and it will measure a real
divergence rather than the harness: the peaceful arm of `Mob.checkDespawn` is
ported on those branches but `EntityType.canSpawn`'s guard in the spawn-egg
path is not, so at `peaceful` ours emits an `add_entity` and a removal a tick
later where vanilla emits nothing at all.

### COLD START — branch `harness`, 2026-09-05 (second entry)

**A fourth thing moved, and it is the difficulty defect's sibling.**
`ci/t2-gameplay-subset.sh` quieted vanilla with the four substrate game rules
and sent nothing to the subject — it passed no `--console` to
`rustmc-server`, so it could not have. Every subset observation this project
has taken compared a random-ticking, clock-advancing, weather-cycling subject
against a quiet reference, in the one instrument whose whole purpose is that a
red seen there is evidence about a red seen in the gate. `--console` reaches
both `rustmc-server` and `record` now, so a `[[gamerule]]` scenario can run in
the subset for the first time.

The list was written out twice, which is how the two scripts could drift, and
`ci/gameplay-substrate.sh` holds it once. **That is a hazard in itself and it
is closed in the same commit**: the reference cache's `harness` component
hashed `ci/t2-gameplay.sh`, and the comment beside the array said "change a
rule, change the substrate, miss the cache, boot vanilla" — a sentence moving
the array would have made false. The key now covers
`ci/gameplay-substrate.sh`, `ci/gameplay-difficulty.sh` **and the value of
`GAMEPLAY_DIFFICULTY`**, which no file hash can see.

`ci/gameplay-substrate-falsify.sh`, in T0, 18 assertions, 10/8 red on the
unrepaired scripts. `ci/reference-cache-falsify.sh`'s fixture gained the two
new files — without them the whole `harness` component reads MISSING and
twelve of its cases fail, which is that suite noticing the key grew.
