# Cold start — branch `corpus-stateid-decoder`

---

### COLD START — branch `corpus-stateid-decoder` off `main` = `15a558cf`, 2026-08-11

**The container state id is now rebased onto the scenario, and all three
`ci/checks/gameplay-unstable.toml` entries that named it are retired in one
commit.** Remote dir `~/rustmc-stateid` on the Threadripper; worktree
`/home/sayer/grit/work-stateid`. Not merged — the orchestrator gates.

    CORPUS FLOOR   7/92 -> 4/92   (ensemble coverage92-selfcheck-n6-20260811)
    PROJECTION     proj:1e154ef8765b7525 -> proj:7cefde0fb56d9f3e
    T0 GREEN, 0 no-vanilla skips, corpus-floor-falsify 21/21

## What was built

`compare::rebase_session_counters` already rebased `seq` and `window` onto the
scenario — the repair written for the `worldgen-iceberg-912` one-sided
reconnect. **`container_set_slot.state` is the third counter of exactly that
class and was the last one still compared raw.** It is now rebased too, scoped
per **(connection, window)** rather than per connection, because
`AbstractContainerMenu.stateId` is an instance field and each menu counts on
its own.

The mechanism was re-derived from the pinned jar rather than taken from the
three entries that assert it — three entries agreeing is three readings of
code, and ORACLE rule 9 says readings of code do not move suspects.
`javap -p -c -constants`, `server-26.2.jar`:

* `AbstractContainerMenu` holds `private int stateId`; `incrementStateId` is
  `stateId = (stateId + 1) & 32767` and **returns the incremented value**;
* `ServerPlayer$1.sendSlotChange` — the `ContainerSynchronizer` — builds
  `ClientboundContainerSetSlotPacket(containerId, incrementStateId(), slot,
  stack)`, and `sendInitialData` calls the same method for
  `container_set_content`;
* `broadcastChanges` walks every slot once a tick into
  `synchronizeSlotToRemote`, which sends only where `RemoteSlot.matches` is
  false.

So the wire value counts **packets sent on that menu since it opened**, not
changes made, and which 50 ms tick sweep saw which intermediate state decides
whether an earlier write cost one packet or two. Every later packet on that
menu inherits the difference. Vanilla cannot reproduce it against vanilla.

## Where it went, and why not in `trace`

**In `compare::project`, not in `trace::decode`, and that is forced rather than
preferred.** `trace::decode` runs at *record* time — `Session::pump` calls
`trace::observe` on each packet as it arrives — so a change there cannot
re-project a recording that already exists. The whole cheap control this item
depends on is re-projecting the surviving N = 6 recordings with no vanilla
booted, and that only works for a change in `compare`. It is also where the
`Normalizer::entity` numbering was moved to, for the related reason that a
transform must see the observations a check actually looks at.

The sound-seed precedent is still the right *shape* (fix the instrument rather
than declare the mechanism uncomparable) — but that one is a `trace` change
because it drops a field from a packet body, and this one is not, because it
rebases a value against other observations in the same projection.

## Rebased, NOT dropped — the control that decides it is the right fix

A decoder that **dropped** the state id would stop all three flapping too, and
it would be wrong: how far the counter moves *between two packets on one menu*
is real behaviour a check may legitimately assert. Six tests pin both halves,
and **both wrong decoders were injected on the box and confirmed to redden
them** — the tests are gates, not decoration:

    drop the field (every state -> 0)   5 fail, incl.
                                        two_streams_differing_in_the_gap_...
    no rebase at all                    7 fail, incl.
                                        two_streams_differing_only_in_session_
                                        history_project_the_same

## Window 0, explicitly

Two different fields on the same observation, and the easy mistake is to treat
them alike:

* the `window` **field** of the player's own inventory is the constant `0`, a
  name and not a draw. **Still left exactly as it was.**
* the `state` **counter** on that same window 0 is rebased like any other, and
  it is the case that pays. `InventoryMenu` is created with the player and
  never closes, so window 0 is the only menu whose counter accumulates across
  the *whole corpus run*. All three retired entries land on it.

`ci/checks/gameplay/README.md`'s requirement that **`790` stay the
highest-numbered check is unaffected and still load-bearing** — it is about
`Player.addItem` advancing the window-0 counter and the free-slot layout for
every later check. The rebase makes the advance uncomparable, not absent.
**Do not renumber it.**

## The measurement — paired, not statistical

Ensemble `coverage92-selfcheck-n6-20260811`: the same six cold-vanilla
recordings at `~/rustmc-cov2/oracle-work/gameplay-selfcheck/r{1..6}.json`, one
recorder (`5b3add3e0a13036b`), 15 pairwise comparisons, `ci/corpus-floor.sh`,
Threadripper, 2026-08-11. **One binary, one variable — the three-element
`COUNTERS` array.** No vanilla booted.

                                                  rebase off   rebase on
    swapping-to-the-offhand-shows-on-both-channels   9/15         0/15
    closing-a-crafting-table-returns-the-grid-...    9/15         0/15
    a-flower-pot-empties-when-right-clicked          9/15         0/15
    distinct projections, each of the three          2            1
    CORPUS FLOOR                                     7/92         4/92

The `rebase off` column reproduces what the standing
`ci/checks/gameplay-floor.toml` already records for all three on this same
ensemble under `proj:b385528330fee646` — 9/15, 2 distinct, `first_difference`
naming `container_set_slot`, window 0, in every case. That is what makes the
treatment column mean anything (rule 7).

**On the power test, because the tool disagrees and the disagreement is
instructive.** `ci/corpus-floor.py` prints `NO LONGER UNSTABLE` for the
crafting table and the flower pot and `UNRESOLVED AT THIS N` for the offhand
swap, because it tests this ensemble against the rate written on each *entry*,
and the offhand entry's `11/25` comes from the N = 26 ensemble the 2026-08-07
disk failure destroyed: `(1 - 11/25)^5 = 0.0551 > alpha`. Against the rate
**this** ensemble itself measures for it — 9/15, twice, by two different trees'
binaries — five runs miss it with probability `0.4^5 = 0.0102`, inside alpha.
And the comparison is **paired, not independent**: same six recordings, same 15
pairs, and the 9 that flapped are among the 15 that now agree. The flap was not
missed, it was removed. This is the same correction the file already makes for
`chest-opens-a-screen`.

All three are removed **as a prediction, not a conclusion**: the selfcheck
verifies the file in both directions, so a run in which the decoder did not
hold reports `NEWLY UNSTABLE` and exits non-zero.

## Two instrument defects found on the way, both fixed, both falsified

Neither was the item and both were on the critical path.

1. **`ci/corpus-floor.sh`'s rule-7 cross-check compared a claim against its
   complement.** It asserted that the set of scenarios whose two declared
   projections differ equals the set `compare` called `!= "match"`. Three of
   `compare`'s verdicts — `Inert`, `Mute`, `Vacuous` — are each guarded on
   `out.verdict == Verdict::Match`, so each asserts the projections are
   byte-**equal** while not being the string `match`. It could not fire while
   every such scenario also differed for another reason; the rebase removed the
   other reason (`container-click-pickup-all-gathers-matching-stacks` went
   `diverge` -> `match` -> relabelled `mute`) and the script refused a correct
   measurement. Now built from the same claim rather than its complement, and
   **stricter**: a mute or inert scenario whose projections genuinely differ is
   now caught.

2. **`ci/corpus-floor-falsify.sh` identified a saved measurement by ensemble id
   alone.** The recordings and the comparator are two independent inputs —
   which is why the floor file has a `projection` stamp at all. A branch that
   changes `compare::project` and measures the same recordings gets a
   `floor.json` whose ensemble matches while describing something the committed
   floor file was not written from; case 1 then adjudicated an already-stale
   committed floor against a fresh measurement and T0 went red on a branch that
   had done nothing wrong. Now keyed on `(ensemble, projection)`, with an absent
   stamp on either side counting as a mismatch. **Falsified both ways** — with
   the projection restamped to match, case 1 still takes the real route and
   still fails on the stale floor, so it is narrowed and not disabled.

## What is NOT claimed

* **The flower pot does not convert and is not claimed to.** Its verdict is a
  real gap on a different field: vanilla sends `block_update` returning the
  block to `minecraft:flower_pot`, we answer `block_changed_ack` and nothing
  else, because `FlowerPotBlock.useWithoutItem` is unported.
* **`swapping-to-the-offhand-shows-on-both-channels` is baselined `absent`** and
  stays there. Retiring the *entry* was the deliverable; converting the
  *verdict* was not.
* **`container-click-pickup-all-gathers-matching-stacks` also stopped flapping
  on this field** and is not claimed as a conversion — it was never declared
  here, so nothing comes out for it.
* **The floor is stale and is NOT quotable.** It is stated beside the score,
  never subtracted.

## Nulls — do not re-run these

* **Do not put this in `trace::decode`.** Decode runs at record time; a change
  there cannot re-project an existing recording, which kills the cheap control.
  Established by reading `Session::pump` -> `trace::observe`, not by trying it.
* **Do not re-measure the three entries against `twin-homog-20260809`.** Those
  recordings are a different ensemble; `coverage92-selfcheck-n6-20260811` is the
  one both the flower-pot entry and the standing floor file were written from,
  and it is the only one that supports a paired before/after.
* **Do not boot the selfcheck for this.** Twelve cold boots and the box-wide
  lock answer nothing the re-projection did not.
* **Do not rename the projected `state` field.** The projection id is a digest
  of the stripped source of `compare.rs`/`scenario.rs`/`trace.rs`, so it moves on
  any semantic edit; renaming buys nothing and would make `state` the odd one
  out beside `seq` and `window`, which rebase in place.

## The single next item

**Re-stamp `ci/checks/gameplay-floor.toml`** — already queued, and deliberately
sequenced *after* this branch so the corpus is stamped once rather than twice.
It is stale in two independent ways now and both must move together:

    [floor].floor      declares 7, measured 4
    [floor].projection proj:b385528330fee646, this tree compares under
                       proj:7cefde0fb56d9f3e

The three retired names must come out of its `[[evidence]]` rows with it. **Do
not re-stamp it from this branch** — that is the `corpus-floor-n6-gpfloors`
mistake, and it is the reason the sequencing exists.

Related hazard for whoever does it: `ci/figures.toml` entries carry an
`artefact_claim`, and one of them was seven characters (`"floor = 7"`) that
survived a re-measure against a different corpus under a different comparator.
Point any claim you touch at something that **cannot** survive a re-measure —
an ensemble or projection id, not a number.
