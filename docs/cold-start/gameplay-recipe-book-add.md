### COLD START — branch `gameplay-recipe-book-add`, rebased onto `gameplay-next-three` (`4a89a2b4`), 2026-08-11

**The item was S2 / `040-stairs-and-furnace-face-apart`. S2 was already ported;
what was missing was a test that reached it.** Originally cut off `main`
(`7ccad044`) on 2026-08-10 and restacked on 2026-08-11 onto the tip of the
three-branch stack. Worktree `~/grit/work-s2recipe2`, remote dir
`~/rustmc-s2recipe`. Not merged — the orchestrator gates. Full account in
`docs/PARITY-ROADMAP.md` §18.

**THE RESTACK WAS ONE REAL COLLISION, NOT A DOC APPEND.** Both branches
refactored the same test helper chain in the same place for different
reasons: `gameplay-next-three` added `play_holding(..., held)` because
`isSecondaryUseActive` is a conjunction with `holdingSomething`, so a fixture
that cannot hold cannot reach the sneaking branch at all; this branch added
`play_tables(..., items, recipe_unlocks)` because the `ServerRecipeBook`
`addRecipes` arm was reached by no test in the tree. **Both reasons are
correct and both survive.** One helper now takes all three, `play` and
`play_with` stay as the narrow front doors, and `play_holding` is gone with
its four call sites re-pointed.

* **Every `None` a re-pointed caller gained is named in a trailing comment**,
  because the failure mode here is a caller that passes one nobody thought
  about: `creative_slot_item` returns `None` without `items` and the fixture
  then reads its own silence as a passing negative control.
* **`held` and `items` are not two spellings of the same thing.** `held`
  writes the hotbar slot directly and needs no table; a `hold` frame resolves
  the numeric id *through* `items` and so *is* the test of `items`. The recipe
  fixtures therefore keep `held: None` and the sneak fixtures keep
  `items: None` — swapping them deletes what the other was proving, which is
  poison P5 below.
* **The unified helper takes eight arguments and that is why the `#[expect]`
  is back.** `clippy::too_many_arguments` warns *above* seven, so the
  seven-argument version was an unfulfilled expectation and a hard error
  (commit `4bb2374e`, T0 exit 101); at eight it is fulfilled and required.
  The restack therefore inverts that commit rather than replaying it, and the
  arithmetic is written on the helper in both directions.
* **`docs/PARITY-ROADMAP.md` §18 sits above §19, not appended after it**, and
  §19's "why 19 and not 18" reservation is corrected in the same commit: half
  of it is now taken, and the outstanding renumbering is
  `corpus-coverage-audit`'s (`4b532a71`) alone.

**WHAT THE TREE REFUTED, BEFORE ANY CODE.** The brief asked for the S2 port —
one packet, one call site, one generated table. All four pieces are on `main`
already: `crates/rustmc-server/src/recipe_book.rs` and the emit site beside
`player::apply_hand` (`fb545fb3`), the generator `ci/dump-recipe-unlocks.sh`
(`78e82189`), and `Session::known_recipes`. §3's rows are **specifications and
carry no status**, which is how a brief written off that table alone can ask
for finished work; §3 S2 now opens with a PORTED marker so the next one cannot.

**WHAT CLOSED.**

* **The emit arm at `world_send.rs` was reached by no test in the tree.** Every
  `Session` fixture there and in `tests/dig_throughput.rs` and
  `tests/plugin_tick_lock.rs` passes `recipe_unlocks: None`, so the outer
  `if let` short-circuits in all of them and the only witness to that branch
  was the 86-scenario corpus — fifty minutes behind a box-wide lock. That is
  the `IcebergFeature` shape.
* **`play_tables` + three assertions**, one per named failure mode in §3 S2:
  `oak_stairs` sends nothing (040 step 0), the first `furnace` sends exactly
  `01 aabbcc 00`, a second `furnace` on the same connection sends nothing.
  `items` is load-bearing in the fixture — without an item table
  `creative_slot_item` returns `None` and *every* case looks like a passing
  negative control.
* **Poisoned and watched red, three ways — one per failure mode §3 S2 names,
  and each fails at its OWN assertion.** Control passes. (A) drop the
  `known.contains` skip -> two bodies where one is asserted. (B) candidates
  ignore the item held -> red at the step 0 control, which is the failure the
  roadmap warns "looks like it works" because the packet is well-formed and
  carries the right recipe at the wrong time. (C) `replace=true` where
  `addRecipes` has `iconst_0` at offset 133 -> red on the last byte. Logs at
  `~/grit/logs/s2recipe-poison-*.log`.

**THE NUMBERS.** Generated `data/vanilla/recipe_unlocks.json` under this tree's
own provenance stamp: **235 items, 189 captured recipe displays**, with
`minecraft:furnace -> ["minecraft:smoker"]` — §3 S2's reading of the
advancement JSON, reproduced by the generator rather than typed. T0 green on
`~/rustmc-recipebook` with **`skipping: no vanilla` = 0**.

**IT GENERALISES — SIX MORE FIELDS, AND THE NEXT ITEM IS NAMED.** `Session`
has 12 `Option` fields and 5 construction sites under test. Six are `None` at
**all five**, so the arm each gates is reached by no test: `placement`,
`shapes`, `sound_types`, `ticks`, `placed_by`, `recipes`. Not "six untested
mechanisms" — the callees have their own module tests — but six instances of
the same one-level-up gap, where the caller's decision to call is unwitnessed.
`sound_types` is the sharp one: it gates **S3**, which §3 now stamps PORTED
and converted. Not fixed here; that is a branch of its own and `play_tables`
is the shape it wants. Poison each one or it is a more expensive way of
testing nothing. §18.

**STATUS COLUMNS ADDED TO §3 AND §6.** Four of the five S-rows were already
closed or closed-null (S1 PARTIAL, S3 and S5 PORTED, S4 CLOSED-null); only
S1's two missing packets remain, and §10 found 530 unreachable under strict
regardless. A spec table with no status reads as a to-do forever — that is
what put an agent on S2 two days after it landed.

**THE `filled_map` WART IS A SEPARATE ITEM.** Not the S2 keying trap, not the
`known` gate, not the emit site: it is the **capture** step in
`cmd_dump_recipe_displays`, where `split_single_entry` can only carve an entry
out of a body whose count is 1. The candidate list is correct
(`misc/map_cloning.json`, criterion `has_filled_map`, rewards exactly
`["minecraft:map_cloning"]`); the live packet was not one entry. The
consequence is that the item is dropped from `by_item` and holding one sends
**nothing** — silence, not a wrong packet. Fixing it means the general
recipe-display codec S2 was scoped to avoid, and it cannot move 040. Left
alone deliberately.

**OUTSTANDING — the corpus verification of 040 has NOT been taken on this
branch.** The corpus lock is held by the agent repairing the recorder, and a
verdict taken underneath that repair is not a verdict. This branch claims T0
and a fixture that has been shown to fail; it does **not** claim 040 closed.
§12 applies to whatever run is taken: 040's step attribution was a coin flip
until `Step::Hold`'s window widened, so one green reading is weaker than it
looks.

**NULLS — DO NOT RE-RUN THESE.**

* **Do not re-port S2.** It is on `main` and its generated table is right.
* **Do not put `#[expect(clippy::too_many_arguments)]` on a seven-argument
  function.** The lint warns *above* seven, so the expectation is unfulfilled
  and `-D unfulfilled-lint-expectations` (implied by `-D warnings`) makes it a
  hard error. T0 exit 101; cost one gate cycle here. **And do not read that as
  "there is no attribute on `play_tables`" — after the restack it takes eight
  arguments, the lint fires, and the attribute is back and required.** The
  rule is the arithmetic, not the verdict: count the list before you touch it
  in either direction, because removing an argument breaks the build as surely
  as adding an unannotated one and neither failure names the cause.
* **Do not chase `filled_map` as an S2 defect.** It is a dumper capture gap
  with no corpus scenario behind it.
