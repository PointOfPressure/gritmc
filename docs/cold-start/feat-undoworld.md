# Cold start — branch `feat-undoworld`

---

### COLD START — branch `feat-undoworld` off `feat-decor` `51326222e`, 2026-08-18

`pale_moss`, and with it both pale oaks. Worktree `~/grit/work-undo`.
`docs/PARITY-ROADMAP.md` §85.

## The thing to know before anything else

**The obstacle §79.6 handed over was half of it, and the smaller half.**

§79.6 said: `tree::place` runs decorators inside an `UndoWorld` implementing two
traits, a nested `ConfiguredFeature::place` wants `FeatureWorld`'s twenty-three,
and the wrapper holds the only `&mut`. All true. That half is 66 mechanical
forwarding methods and it is generated, not written.

The half that costs a day is that **`UndoWorld` is generic**, so once a
decorator can place a feature the wrapper type recurses without bound:

```text
  tree::place::<Decoration>
    -> ConfiguredFeature::place::<UndoWorld<Decoration>>
      -> tree::place::<UndoWorld<Decoration>>          a nested feature can
      -> ...<UndoWorld<UndoWorld<Decoration>>>         itself be a tree
```

It surfaces as `error[E0275]: overflow evaluating the requirement`, **reported
from `worldgen-assembly`, not from `rustmc-worldgen`**, naming a type 127 levels
deep and suggesting you raise the recursion limit. Raising it does not help. No
arrangement of bounds or `&mut` helps, because each level is a different type.

The answer is to stop monomorphising at that boundary: the feature-placing
chain takes `&mut (dyn FeatureWorld + '_)`. `dyn FeatureWorld` implements all
twenty-three supertraits, and every feature's own `place` was already `?Sized`,
so nothing else moved.

> If you are re-costing anything in this area, cost the recursion. The `&mut` is
> the part that looks hard and is not.

## Two things that will cost you a compile each

* **The `+ '_` is not decoration.** `&mut dyn FeatureWorld` in argument position
  defaults its object lifetime to `'static`. Both real worlds borrow. Without
  the explicit `+ '_` you get *"borrowed data escapes outside of closure"* on
  the vegetation-patch callback in `configured.rs`, which is nowhere near the
  signature that caused it.
* **`min_y`, `max_y`, `block_at`, `set_block`, `is_air` and `face_is_sturdy`
  each appear in three or more of the twenty-three traits.** The moment a world
  is bound by `FeatureWorld` rather than `TreeWorld`, every bare `world.min_y()`
  in that function becomes `E0034: multiple applicable items in scope`. Write
  `<W as ThatTrait>::method(..)`, or for the dyn chain
  `<dyn FeatureWorld as DecoratorWorld>::method(..)`.

## The rule that keeps `UndoWorld` correct

Twenty of the twenty-two delegated traits declare a `set_block`. **All of them
forward to `<Self as TreeWorld>::set_block`, never to `self.inner`.** That is
the one path that records the previous state, so the undo log covers a nested
feature's writes.

Pointing any one of them at `inner` compiles, places the right blocks, and
passes everything that existed before this branch, while leaking that feature's
blocks past a rollback. `a_rollback_undoes_a_nested_features_writes_too` is
there to refuse it and has been falsified against exactly that mutation.

Defaulted methods are delegated, not inherited: three of them are overridden by
the real `Decoration` world, and inheriting a default would answer a different
question inside a tree than outside one.

## Nulls — do not re-do these

* **Do not relax `R: RandomSource` to `?Sized`.** It looks necessary because
  the decorator path carries `&mut dyn RandomSource`. It ripples into nineteen
  signatures and then into theirs. `crates/rustmc-worldgen/src/rng/mod.rs::RandomSource` (`:50`) is already blanket-implemented, a few lines below its trait, by an impl that
  `RandomSource for &mut T` where `T: ?Sized`, so `&mut dyn RandomSource` is
  itself a sized `RandomSource` and one extra borrow at the call site does it.
* **Route B (decorators outside the wrapper) is refused, with a reason.** It
  reintroduces the failure the wrapper was built for: decorators run before
  `update_leaves`, so a decorator refusal outside the wrapper leaves a built
  tree with no leaf-distance pass. That was 4,049 blocks of undecayed spruce.
* **The registry is a parse-time problem, not a place-time one.** Vanilla looks
  `PALE_MOSS_PATCH` up off `level.registryAccess()` inside `place`. The name is
  hardcoded and its codec carries only three probabilities, so it resolves once
  while the tree config is read. Threading a registry to place time means a
  registry parameter on twenty-three world traits.

## A test that passed its own falsifier

`a_hanger_is_a_contiguous_run_with_exactly_one_tip`, first version: real seeded
stream, asserted non-empty / contiguous / one tip. Passed. Then passed the
mutation it was written to catch, because on that seed the loop never ran, the
hanger was one block long, and every assertion held either way.

It is scripted now. **A real random source is not coverage**, and in this area
the branch you care about is often the one a given seed does not take.

## What is measured, and what is emphatically not

```text
  TREE CONFIGS: 32/39 supported     (30 before)
  lib unit tests                     834 pass
  workspace                          builds, clippy clean apart from the
                                     huge-mushroom warning already on the base
```

**No world was generated on this branch.** No census, no draw comparison, no
gate. The box was needed elsewhere. `pale_garden`'s 9,129 missing blocks in
window A are **not** measured and **not** claimed here.

What is safe to say: `pale_garden` placed zero trees because both
`pale_oak_checked` and `pale_oak_creaking_checked` refused, and both now report
supported. Whether that closes the 9,129, part of it, or exposes the next
defect is unknown until someone takes a **block census** — not the draw table,
and not window A's ratio, because `pale_garden` at 0.52x and `dark_forest` at
0.67x cannot adjudicate their own ports and "N of M identical" from the draw
oracle is *draw*-identical.

## Environment

Both repairs from `feat-decor`'s cold start still apply and both still bite:
`cargo` is at `~/.cargo/bin` and not on the non-login PATH, and a piped
`cargo build | tail` reports exit 0 when cargo is missing entirely, so check
that real compile output appeared. A fresh worktree's `data/vanilla` must be
copied from a live worktree, not from `~/grit/repo`, whose copy predates
`randomlyTicking`. `oracle-work/*` can be symlinked.

`ci/unlanded.sh`, `ci/run-detached.sh` and `ci/claim-number.py` are not on
`main`; copy them untracked. `.git/info/exclude` already covers the first two.

`Cargo.lock` on this family is byte-identical to `main`'s at 2183, so a cargo
run does not dirty the tree and no rebase was needed for it.

## THE SINGLE NEXT ITEM

**Take the block census for `pale_garden` and say what the 9,129 became.** It is
the one number this branch could not produce and the only thing that settles
whether `pale_moss` closed the entry or uncovered the next one.
`ci/decorate-once.sh`'s vectors already carry vanilla's blocks, so it is one run
a side.

Second: nothing exercises a nested-feature rollback end to end. A tree whose
patch is unported, placed into a real chunk, should leave that chunk
byte-identical. `pale_moss` is the first and only path where that can happen.
