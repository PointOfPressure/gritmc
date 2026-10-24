# `claims-allocator` — section numbers are allocated now, and three kinds of evidence were rotting

Read `docs/PARITY-ROADMAP.md` §59 for the full account. This is what the next
agent needs before touching the claims registry.

## Read this first: care is not sufficient, which is why the allocator is not advice

On 2026-08-18 a lane hit a section-number collision, resolved it by hand, and
did everything the process asked. It looked at what it could see. It honoured
the forward reference — a §32.3 citation pointed into `worldgen-isolated-drift`,
so that side kept 33. It avoided 36-39 because two other lanes had claimed
them. It took the next number that looked free.

**It collided again three minutes later, and the original collision is still
open.**

* The number it moved to, 46, **had been taken at 10:07:36** by the
  biome-census branches under `clamp-does-not-clamp-the-nether`.
  `worldgen-land` chose it at **10:10:15**. Two and a half minutes, and no
  instrument on this box could have told it.
* And the move landed on `worldgen-land` **only**. `worldgen-aggregate` still
  holds 33 on its own branch under the same slug — one claim, two numbers, two
  branches — so the collision the renumber was performed to end is still live
  on the branch the claim is named after.

Nobody was careless. The information needed was on a branch nobody involved
could see, and the only instrument for it was asking the other lanes, which
does not scale and is stale by the time it is answered.

So: **`ci/claim-number.py --claim` is not a tidier way to do what you would
have done carefully. It is the only way to get an answer that is true when you
act on it.** And `ci/renumber-claim.sh` exists so a move cannot be
half-applied, which is the second half of what went wrong above.

`ci/claim-number.py --check` runs in `ci/t0.sh`, so this is enforced rather
than recommended.

## A BOX-WIDE RULE, not a note about one script: never move a branch ref another worktree is sitting on

**This applies to anything on this box that commits to, resets, or force-moves
a branch — not just to the claims registry.** It is written here because this
is where it was found, not because it is scoped here.

There are ~250 worktrees over **one** ref store, and more than one can hold the
same branch. `git worktree add --force` will hand you a second checkout of a
branch that is already checked out somewhere, without complaint.

The moment you commit, the branch ref moves. The **other** worktree is then
sitting on your new commit as its HEAD with the **old tree still in its
index** — so `git status` there shows your change staged as a **revert**. A
lane that commits from that state undoes your work, and **neither of you sees
it happen**. There is no conflict, no warning, and no error.

`~/grit/work-end` was in exactly that state after the DETERMINISM §48
renumber, with both claim files staged back to their old names. It was found
by tidying up, not by any check.

**What to do.** Before you move a branch ref, list the worktrees holding it:

    git worktree list --porcelain | awk -v b="refs/heads/$BRANCH" \
      '/^worktree /{w=$2} $0=="branch "b{print w}'

A co-checkout whose `git status` is **empty** can be brought forward with
`git reset --hard $BRANCH` after your commit. One with **anything** of its own
must be left alone and its path reported — that is a lane's uncommitted work
and it is not yours to discard. `ci/renumber-claim.sh` does both automatically
and `ci/renumber-claim-falsify.sh` has a case for each half; copy that shape
rather than reinventing it.

The subtlety that cost me a second attempt: the safety check has to run
**before** the commit. Afterwards every co-checkout looks dirty, because the
staged revert *is* the damage — a post-commit dirtiness test skips exactly the
worktrees it was written to repair.

## The one thing to change in how you work

    ci/claim-number.py --claim -d PARITY-ROADMAP -s my-finding -t "What I measured"

prints a number, reserves it atomically against every other worktree on this
box, and writes the claim file with `status = "reserved"`. Write the `## N.`
heading, set `status = "written"`, commit both together.

`ci/claim-number.py --show` is who holds what. `--check` refuses if this tree
collides with any ref, which is the thing `ci/check-doc-claims.py`
structurally cannot do — that script adjudicates **one tree**, so a
cross-branch collision is invisible to it until both sides have already
merged into one.

## Six collisions, three closed, three that need you

**Closed** with `ci/renumber-claim.sh`, which moves the claim file, its
`number` field, the `## N.` heading, every `### N.M` subsection, every `§N`
citation and the cold start that quotes it — in one commit, **on top**, never
rewriting, so a lane waking on the branch has lost nothing:

| § | moved | to | clause | branches touched |
|---|---|---|---|---|
| 45 | `sound-pitch-gradeable` | 60 | 2 | corpus-sound-pitch, corpus-projection-audit, area-projection (local only, not pushed) |
| 51 | `gamerules-console-closes-grass-dirt` | 61 | 1 | gamerules-console |
| 58 | `end-is-a-coverage-item` | 62 | deviation | end-biome-census |

**Open, and the verdict is already computed** — each is one command once the
lane is free. In all three the side the rule moves is chained or live, and
rewriting one of those costs a lane its work to save an orchestrator a
renumber:

    ci/renumber-claim.sh PARITY-ROADMAP gameplay-motion-combat motion-and-combat-rows-were-stale 53 <new> "<reason>" --push
    ci/renumber-claim.sh PARITY-ROADMAP gameplay-corpus-gaps   note-block-is-three-defects       54 <new> "<reason>" --push
    ci/renumber-claim.sh PARITY-ROADMAP proto-layer            serverbound-protocol-layer        57 <new> "<reason>" --push
    ci/renumber-claim.sh DETERMINISM    worldgen-aggregate     worldgen-aggregate                33 <new> "<reason>" --push
    ci/renumber-claim.sh DETERMINISM    worldgen-land          worldgen-aggregate                46 <new> "<reason>" --push

**`<doc>` is a required first argument.** An unqualified `§N` means whichever
of the two registered documents it is written in, so a rewrite that does not
know which one it is moving can renumber a citation in the other.

(That sentence used to spell the point out with a real number and both
filenames, and `check-doc-claims`'s DANGLING-SECTION-REF fired on it — its
qualified-citation rule pairs a document name with any `§N` within 24
characters, and a sentence naming both documents and one number is genuinely
ambiguous to it. The check is right and the sentence was the problem.)

§53's slug is also on `gameplay-corpus-gaps` and the `wip/gpland/*` landing
branches; §57's *other* side spans four branches, which is why the rule's
answer is also the cheap one there. Take the new number from
`ci/claim-number.py --claim`, never by hand.

## DETERMINISM has its own collisions, and I nearly missed all of them

I measured `PARITY-ROADMAP` and reported. `--check` on a renumbered branch
then named two more, in the other registered document. **Run `--show` for both
docs; the registry covers two and the interesting one is not always the one
you are working in.**

`DETERMINISM §48` is closed (`end-references-were-destroyed` -> §51, same
deviation and same reasoning as PARITY-ROADMAP §58). `§33` and `§46` are
`worldgen-*` and not mine to move.

`33` and `46` there are the incident at the top of this file. They are
`worldgen-*` and not mine to move; the commands are above.

**THE RULE, in the order it applies.** Record which clause fired in the commit
— `renumber-claim.sh` requires it as an argument — because the point of a
written rule is that the next collision is a lookup and not an argument:

1. the side with **no forward reference into it** moves;
2. where both are referenced or neither is, the side on the **newer branch**
   moves, newer by **when the claim file was born**, not by last commit;
3. where that still ties, the side whose **prose is shorter to repair** moves;
4. **PERMITTED DEVIATION — do not re-argue this one.** Where the side clause 2
   names is carried by more branches, or by a live or chained one, and the
   other side is a **sole carrier on a branch you are cleared for**, move that
   one instead and say so in the commit. Clause 2 exists to minimise
   disruption; following it literally when it means rewriting two live
   branches instead of one idle one defeats the thing it is for. Both
   deviations taken on 2026-08-18 (PARITY-ROADMAP §58, DETERMINISM §48) were
   reviewed and accepted on exactly that reasoning, and each is reversible in
   one command if the other side turns out to be free.

**Do not delete clause 1 as redundant.** It looks like it never fires, and it
fired once out of four — on §51, where it decided *against* the branch I would
have picked on age alone. `proto-layer`, which holds neither slug, cites §51
meaning "what the corpus projection removes", which is a reference into
`corpus-projection-audit`'s side and not into `gamerules-console`'s. Checking
it costs one `git grep`.

## The state that started this



Found the first time `--show` was pointed at the real ref store, all under
different slugs, none detected by anything:

| § | one claimant | the other |
|---|---|---|
| 45 | `menu-reachability` — `gameplay-container-menus`, `gameplay-menus-round2` | `sound-pitch-gradeable` — `corpus-projection-audit`, `corpus-sound-pitch` |
| 51 | `gamerules-console-closes-grass-dirt` — `gamerules-console` | `projection-audit` — `corpus-projection-audit`, `corpus-sound-pitch` |
| 54 | `note-block-is-three-defects` — `gameplay-corpus-gaps` | `what-the-corpus-never-asks` — `corpus-coverage-gaps` |
| 57 | `nether-is-two-bastions` — `nether-biome-census` | `serverbound-protocol-layer` — `proto-layer` |

**§54 is the one recorded as resolved by hand on 2026-08-18. It is not
resolved.** Both branches hold it today. Whoever lands the second of any pair
above gets `DUPLICATE-CLAIM` and pays for the renumber; running
`ci/claim-number.py --check` on those branches now costs nothing and moves the
cost to the lane that can pay it cheapest.

**These are not mine to renumber** — every branch above belongs to a live
lane. The tool that finds them is the deliverable; the renumbering is the
holders' call, and §32.3-style forward references mean picking which side
moves is a judgement about the prose, not about the number.

## Why `refs/heads` and not just `origin`

Claiming a number for §59 returned **59, not 58**: `end-biome-census` holds
§58 on a local branch that has never been pushed. An allocator built on
`git ls-remote` — the obvious implementation, and cheaper — would have handed
out 58 and made a fifth collision while reporting success. If you rewrite this
tool, keep the local-heads half.

## The lock is git's, not one I built

All 250 lane trees under `~/grit/` are worktrees of one repository and share
**one ref store**. `git update-ref <ref> <new> ""` — empty old-value — means
"create this, fail if it exists", under the ref lock. That is an atomic
compare-and-swap that was already there. A reservation is
`refs/claims/<doc>/<number>` pointing at a blob naming the claimant. Six
concurrent claimants in the falsifier get 1..6: six distinct numbers, none
skipped.

Scanning and adding one is most of the answer and is what `--next` prints. It
fails in exactly the window that produced the two §33s — two lanes scanning
inside the same fetch interval both compute the same number and are both
correct about a fact that stopped being true while they were writing.

## `status = "reserved"` is not a hole

The registry could not express "taken, not written yet": a claim file with no
heading is `CLAIM-NO-HEADING`, a red. So it **refused the honest act** and
permitted the dishonest one. `reserved` is exempt from `CLAIM-NO-HEADING` and
from nothing else — fully counted by `DUPLICATE-CLAIM`, and
`RESERVED-BUT-WRITTEN` reddens the moment the heading appears.

## Three kinds of evidence, all rotting, all now gated

| what | instrument | what it found |
|---|---|---|
| source citation | `ci/check-source-citations.py` | 19 stale, one by 670 lines; a CLOSED row cited a comment about thread locals |
| prose count | `ci/scenario-census.py --check` | `720` said 34, measured 39 steps / 15 scenarios; `4 of 84` is `4 of 90` |
| commit sha | `ci/check-cited-commits.py` | 20 of 106 cited shas reach no ref at all |

The source-citation checker **needs no ledger**, and that is the design point
worth carrying forward: `` `file.rs::symbol` (`:line`) `` states its own
oracle, so the check is whether the two halves agree and there is no third
artefact to drift. Prefer that shape wherever it is available. Where it is not
— a prose count cannot say what was counted — keep the ledger **inline with
the code that derives it**, because a ledger in a separate data file is a
second artefact and three of 2026-08-18's corrections were exactly that.

## The distinction that will bite you next

**A quotation of a past claim is not a claim.** Nine occurrences of "of 84"
were deliberately left wrong: `worklist-merge-driver.md`,
`docs-figures-ledger.md` and `ci/merge-docregions.py` quote the strings
`10 of 84` and `4 of 84` as the evidence in an account of a union-merge that
published one cold start twice. Updating a quotation to a number that was not
in it when it was quoted destroys the only record of what happened.

`ci/check-source-citations.py` met the same problem and reddened on the
section documenting itself, because §59.4 quotes a CLOSED row's wrong line
numbers *as the evidence*. The answer there is a form the document uses
directly — `` `file.rs::symbol` (was `:856`) `` — counted and printed on
every run rather than behind `--list`, because an exemption whose size is
invisible is an exemption that grows. There are two and both are in §59.4.

## Things measured and found STILL TRUE, so nobody re-derives them

* §17's "`Step::Look` repaired to 250 ms" — `tools/oracle-runner/src/client.rs`.
* `520`'s `collapse_repeats = ["move_entity_heartbeat"]`.
* The combat row's caveat: `550` and `551` both carry `damage_event`,
  `set_health` and `entity_event` in `observe`.
* The container-click row's "no scenario in the corpus exercises `QUICK_CRAFT`
  or `CLONE`" — and it is **stronger than stated**: no scenario sets a click
  `mode` at all, so every `container_click` in the corpus is the default.
* All 17 check ids named across the CLOSED rows exist.
* `click.rs:794` still lands on the `QUICK_CRAFT`/`CLONE` refusal.

## Do not size a branch with `git cherry` alone either

`docs-queue-order-race` measured **1 patch ahead** and is fully landed in
substance — its cold start and `WORKLIST` row are on `main` byte-identical and
its `docs/CI.md` is **older** than main's. Acting on the number without
looking would have reverted main's newer `docs/CI.md` and deleted five cold
starts. `ci/unlanded.sh` documents this blind spot; this is an instance of it
that would have done damage. `merge-driver-coverage-audit`'s two commits were
genuinely unlanded and are folded into this branch.

## Next items, in order of what they would buy

1. **§53, §54 and §57** — the three commands are above, verdicts precomputed.
2. **Run `ci/check-cited-commits.py --fix` routinely**, not when the ratchet
   complains. A cited sha stops resolving whenever a branch rebases, and one
   did so *during* the session that wrote this: `clamped-floor-probe` rebased
   and the patch moved off `11ff71d8`. `--fix` follows it by patch-id, which
   is a measurement and not a guess. That is the answer to drift here, not a
   higher `MAX_UNREACHABLE` — leave it at 5.
3. **128 bare `file.rs:1293` citations remain uncheckable.** Converting one to
   `file.rs::symbol` (`:line`) is a strict improvement any lane can make in
   passing; `--list` prints them.
4. `720`'s "vanilla sends `set_cursor_item` 221 times across eight scenarios"
   is unverified — it needs the corpus lock, and is listed under
   `NEEDS-CORPUS` rather than quietly skipped.
