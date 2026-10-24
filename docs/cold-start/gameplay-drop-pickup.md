# Cold start — branch `gameplay-drop-pickup`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `gameplay-drop-pickup` off `main` (`78e82189`, rebased onto `05c9a2d3` then `51fa7c62`), 2026-08-08

**Item: S4, `docs/PARITY-ROADMAP.md` §6 rank 8, the drop and pickup chain.
CLOSED AS A MEASURED NULL. `050` cannot convert and neither can the four it
was said to unblock.** Worktree `~/grit/work-droppickup`; box work in
`~/rustmc-droppickup` and, for one throwaway experiment, `~/rustmc-dp-exp`.
Not merged — the orchestrator gates.

**Rebased mid-item and the rebase was not free**, which is worth one line
because a clean rebase reads as a green one: `docs/PARITY-ROADMAP.md` gained a
§10 on `main` so this item's account is **§11**, `docs/WORKLIST.md`'s
cold-start list conflicted, and `Entity` gained a `synched` field that the
rebase merged into `spawn_thrown_item` but could not reach in a hand-built
test thrower — caught by T0, not by git. The instrument moved too:
`tools/oracle-runner/src/trace.rs` gained 52 lines, so every measurement below
was re-taken on the rebased tree and every one reproduced to the line.

**THE NUMBERS, first.** Full corpus, `ci/t2-gameplay.sh`, seed 20260803,
sayer-desktop, 2026-08-09 02:19-03:11, taken on this branch at `2fea3d78` —
`main` at `05c9a2d3` plus this branch, i.e. **before S3 landed**:

```
before (main 78e82189 / 05c9a2d3):  declared 60/86, strict 57/86
after  (this branch, same base):    declared 60/86, strict 57/86
```

Unchanged, as the prediction registered in `f1ba3797` before the run said it
would be. `050` did not convert (declared `match`, strict `diverge`, exactly
where it was); `350` `absent`/`diverge`, `500` `diverge`/`diverge`, `605` and
`606` `diverge`/`diverge`, all unmoved; **`070` held** at `diverge`/`diverge`
with `we never sent: add_entity`, and `010` held at `match`/`match`. S3 landed
after this run and took strict to 58/86 by converting `080`; **that +1 is S3's
and is not in this figure on either side.** The five `BETTER` lines for
`container-click-*` are `main`'s and are deliberately not ratcheted here.

**Nothing here was ported, and that was the finding rather than a shortfall.**
The measurement that closed it took about an hour, needed no corpus lock and no
new recordings, and is the first thing to reach for the next time an item's
target is an entity: **three full-corpus vanilla recordings already existed on
the box** from that evening's other runs, and `ci/selfcheck-recompare.sh` will
compare saved recordings under today's comparator in seconds.

```
N = 3   declared 77/86 reproduced, strict 75/86
N = 2   the same, over the two recordings whose trace.rs is byte-identical
```

* **`050-torch-falls-with-its-support` is strict-unstable 2/2.** Vanilla fails
  it against itself. Two of three runs let the torch drift out of reach and
  sent `set_entity_motion` · `entity_position`; the third picked it up and sent
  `take_item_entity` · `container_set_slot` · `remove_entities`. **The RNG
  decides which packets exist, not just what is in them**, which is a stronger
  trap than §3 S4 ranked and is why no projection change reaches it.
* **`605` and `606` are newly measured unstable on the gated denominator** and
  are now declared in `ci/checks/gameplay-unstable.toml` with the mechanism.
  `350` and `500` were already declared. `070` reproduced exactly in all three
  runs and is untouched — it is the control and it held.
* **§3 S4's projection change was built and measured rather than argued
  about, in two forms.** The one §3 S4 actually costed — drop `add_entity.at`
  for item entities and `set_entity_motion.v` — is a **no-op**: output
  identical to the unmodified comparator, not one line moved, because `rest`
  still holds the yaw. The maximal form, which also drops `rest`, moves 050
  from strict-unstable 2/2 to 1/2 (the pair that agrees about the pickup
  converts; the pair that disagrees does not) and makes **605/606 reproduce
  2/2 on declared**. Neither is committed. The four-way table is
  `docs/PARITY-ROADMAP.md` §11.

**What did land, and it is fidelity and not a conversion:**
`crates/rustmc-server/src/dropitem.rs` announced every dropped item at **yaw
zero**. `ItemEntity(EntityType, Level)` offsets 39-52 are
`setYRot(this.random.nextFloat() * 360.0F)`. The module's own comment denied
this in as many words and claimed to have checked the bytecode, while
`ci/checks/gameplay-unstable.toml` had recorded the same call correctly since
2026-08-05 — **a fact held in one file and denied in another**. Also fixed: the
item's generator advances by six draws and not two (2 `nextLong` for the uuid,
2 `nextFloat` for `bobOffs` and the yaw, 2 `nextDouble` for the constructor
delta), the uuid came from the *thrower's* generator with its halves swapped,
and `thrower_random` became `unique_random`, a real port of
`RandomSupport.generateUniqueSeed` — uniquifier `8682522807148012`, multiplier
`1181783497276652981`, xor the clock — because the clock-only version is now
called twice per thrown item.

**KILLED — do not re-derive:**

* **S4 does not convert 050.** Measured twice, once by the instrument's own
  aggregator and once by isolating the mechanism with a projection experiment.
* **No exemption of `add_entity.at`, `add_entity.rest` or `set_entity_motion`
  rescues 050.** The maximal version was built and it does not.
* **Porting `Block.popResource` converts nothing.** The bytecode account is
  correct as corrected in §11 and the work is still worth doing for a server
  that should drop items; it must not be booked against 050, and it carries an
  unmeasured regression risk — a new item entity in every cascade window leaks
  into the *neighbouring* scenario's box, which `compare::place`'s doc comment
  already records 500's item doing at 12 of 12 and 34 of 42.

**Reported and deliberately not absorbed, because they are not this item's:**
`a-player-that-leaves-the-ground-is-resynced` (2/2) and
`closing-a-crafting-table-returns-the-grid-contents` (1/2) both came back
`NEWLY UNSTABLE` in the same run and neither is in the declared file.

**The single next item, and the result that closes it.** `605`/`606` are the
only reachable thing S4 leaves behind, and they need **both** halves: a
comparator exemption that drops `at` and `rest` from an item `add_entity`
(reading `minecraft:item`'s id out of `data/vanilla/entity_types.json`, **not**
the hardcoded 71 the experiment used, and in `project`'s `map`, never in
`decode`), **and** the server work to spawn an item entity on a container
throw, which this server does not do at all. Closed by 605 and 606 moving from
declared `diverge` to declared `match` in a full corpus run, with `070` and
`010` unmoved. Neither half converts anything alone, so do not land the
exemption on its own: it moves every scenario's denominator and buys nothing.
