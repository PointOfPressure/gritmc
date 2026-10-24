# Cold start — branch `water-spreads-cascade`

Every `### COLD START` entry ever written for this branch, oldest and newest alike, moved out of `docs/WORKLIST.md` by the doc-collision-class restructure (1 entry). Order matches how they appeared there — not necessarily chronological, since WORKLIST inserted each new one above the last.

---

### COLD START — branch `water-spreads-cascade`, off `main` (`7ccad044`), 2026-08-10

**The `worldgen-iceberg-912` gate's 40 REGRESSED lines were 2 events and 38
shadows.** Worktree `~/grit/work-water`, remote dir `~/rustmc-water`. Not
merged — the orchestrator gates.

**WHAT THE +31 ACTUALLY WAS — and the brief's framing of it is refuted.**
The brief said to find "the one thing that consumes 31 sequence numbers and is
not reset". **Nothing consumes 31.** Both sides stood at `seq` 31 entering
`water-spreads` (scenario 20 of 88). It errored on the **vanilla side only**;
`record::record` drops the party on any error, the next scenario logs in
fresh, and `Session::seq` starts again at 1. Ours never failed, never
reconnected, and kept counting. Vanilla resumed at 2, ours at 33. The +31 is
the distance between a counter twenty-one scenarios old and a brand-new one —
**a one-sided reset, not a consumer.** The same reconnect reset the server's
`containerCounter`, which is the second offset nobody had noticed: 26 checks
moved on `seq` and **12 more on `window`**, which `menu.rs` had already
warned about in the abstract ("pointing at four healthy checks instead of the
one missing block").

**THE NUMBERS.** `oracle-runner compare` re-adjudicating the iceberg gate's
**own two saved traces** — fixed input, only the projection changed, so this
is the poison test with the poison still in place and zero server boots:

```
                     declared      strict     REGRESSED
  gate log, before   26/88         24/88      40
  rebasing compare   63/88         58/88       3
  healthy control    64/88         60/88       0   (3 trees, all identical)
```

The healthy control is the load-bearing one: `~/rustmc-batch2`, `~/rustmc-cov2`
and `~/rustmc-blueice` all re-adjudicate to **exactly** `gameplay-baseline`'s
64 match / 18 diverge / 6 absent. The fix restores the true state; it does not
manufacture matches. And 63/58 differs from 64/60 by precisely the two real
events — `water-spreads` absent→error, `a-player-says-something` match→diverge
— with every one of the 38 back to `match` **without any of them being
touched**.

**THE LIVE RUN, which is the claim the merge gate actually needs.** The table
above is trace replay — a strong control on the projection, but not evidence
that a fresh recording reaches a verdict. `ci/t2-gameplay.sh` on this branch,
Threadripper, `RUSTMC_WORKERS=6`, corpus 88, tree content of `a7324d51`:

```
  CORPUS  declared  64/88 match  (18 diverge, 6 absent, 0 vacuous, 0 inert, 0 error)
  CORPUS  strict    60/88 match  (25 diverge, 3 absent, 0 vacuous, 0 inert, 0 error)
  GAMEPLAY GREEN                 T2-GAMEPLAY GREEN                0 REGRESSED
```

**Exactly `gameplay-baseline`'s 64 / 18 / 6, and zero errors** — so
`water-spreads` recorded normally on this run, which is the 32-in-33 behaviour
measured above and not a repair of anything. Re-adjudicating those same
traces against the shipping 87-scenario corpus gives **declared 64/87, strict
60/87**; against a restored 88 it gives 64/88 and 60/88. The whole delta is
`water-spreads` leaving the `absent` bucket, which is what parking a check
that never adjudicated is supposed to look like.

**T0 GREEN** on `41922c63` in `~/rustmc-water`, `t0-remote` exit 0,
`skipping: no vanilla` count **0**.

**WHAT IT DOES NOT REPAIR.** Rebasing fixes *values*, not *packet sets*. A
reconnected side re-sends its inventory, so `strict` still sees observations
one side has and the other does not:
`a-furnace-opens-a-screen-a-chest-does-not` remains `strict: diverge` on those
traces while its declared projection is two observations agreeing field for
field. **Declared — the gated number — is clean. Strict is bounded, not
abolished.**

**`water-spreads` — OUTCOME TAKEN: PARKED IN `open/`. A QUARANTINE, NOT A
CLOSURE**, and not because it is broken:

* the brief's "cannot be run at all — on either side" is wrong twice. The
  gate itself recorded `0 could not be run` against rustmc; and over the **33**
  corpus `vanilla.json` recordings on the box it records cleanly **32 times
  and errored once** — that once being this gate. Alone against a fresh
  vanilla it recorded 1/1;
* it is parked for what it *cannot do*: baseline `absent`, never adjudicated,
  and **24 flaps in 25 comparisons at N = 26**, so it never will under the
  current projection. `Verdict::Error.baselineable()` is `false` by design, so
  its ~3% error can never be blessed and reds a gate every time it fires.

Corpus **88 → 87**. **This collides with `origin/corpus-coverage-audit`
(`4b532a71`) by construction** — its 88→93 work is not in main; whichever
lands second re-derives the size.

**NULLS — DO NOT RE-RUN THESE.**

* **The `use_item` packet is not malformed and the ids are not wrong.**
  `use_item` = 67, `use_item_on` = 66, `player_action` = 41,
  `set_carried_item` = 53, all confirmed against
  `generated/reports/packets.json`. `water-spreads` is the *only* scenario in
  the corpus that uses `use_item`, which makes it look like an untested path;
  it is not the cause.
* **Do not chase `a-player-says-something` as a regression.** 28 of 29
  `vanilla.json` recordings on the box carry `player_chat` for it; only the
  iceberg gate's produced zero observations. It is a one-off on the reference
  side, not a defect, and not the cascade — it sits at index **13**, seven
  scenarios *before* `water-spreads`.
* **Do not look for a cause inside `water-spreads`' own packets.** It runs
  clean standalone; vanilla's log gives no reason, only
  `oracle lost connection: Disconnected`. The recorder now lifts the
  disconnect reason off the wire (`client::printable_reason`) so the next
  occurrence is diagnosable — it was being thrown away.

**STALE BY CONSTRUCTION, AND DELIBERATELY NOT RE-MEASURED.**
`ci/checks/gameplay-floor.toml` is stamped `proj:58380c9e4cbeae81`; the
projection is now `proj:941413ea3acd112c` and the corpus is 87 rather than 88,
so it will print `STALE PROJECTION`. That is an annotation only —
`ci/t2-gameplay.sh:500` ends in `|| true` and nothing in the floor file can
move a verdict. **The next item on this track is to re-measure it** over
`gpfloors-homog6-20260809`'s six recordings under the new projection and
re-stamp with `ci/corpus-floor.py update`. Expect the floor to move: it stood
at 7 over 88 scenarios, and `water-spreads` — 24/25 flaps — is exactly the
kind of name a floor counts, so check whether it was one of the 7 before
reading any change as the rebase having tightened the instrument. **Those are
two different causes and they are not separable after the fact**; take the
floor over the same six recordings twice, once with the parked scenario
restored, if the difference is going to be quoted.
