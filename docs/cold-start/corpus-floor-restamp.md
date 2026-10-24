# Cold start — branch `corpus-floor-restamp`

---

### COLD START — branch `corpus-floor-restamp` off `main` = `7fa6b7d8`, 2026-08-11

**Re-measured and re-stamped the gameplay corpus floor, which `corpus-stateid-decoder`
had made unquotable at the moment it merged.** Remote dir `~/rustmc-floor` on
the Threadripper; worktree `/home/sayer/grit/work-floor`. No server code
touched. Not merged — the orchestrator gates.

    CORPUS FLOOR   7/92 -> 4/92   (ensemble coverage92-selfcheck-n6-20260811)
    PROJECTION     proj:b385528330fee646 -> proj:7cefde0fb56d9f3e
    corpus-floor-falsify: 21 passed, 0 failed

## What was done

`ci/corpus-floor.sh --update` against the same six `coverage92-selfcheck-n6-20260811`
recordings in `~/rustmc-cov2/oracle-work/gameplay-selfcheck/` — no vanilla
booted, this only re-projects existing recordings through the current binary.
`oracle-runner projection-id --explain` confirmed the tree compares under
`proj:7cefde0fb56d9f3e`, matching the brief and matching `corpus-stateid-decoder`'s
own paired control (`d6112fd9`) exactly, name-for-name: three scenarios leave
the floor (`a-flower-pot-empties-when-right-clicked`,
`closing-a-crafting-table-returns-the-grid-contents`,
`swapping-to-the-offhand-shows-on-both-channels` — all three `container_set_slot`
state-id entries the decoder retired) and the remaining four
(`a-dropped-item-goes-into-your-inventory`,
`a-player-that-leaves-the-ground-is-resynced`,
`dropping-an-item-throws-it-on-the-floor`, `spawn-egg-makes-a-mob`) hold their
exact pre-measure rate. No fourth name moved either way. Full table:
`docs/PARITY-ROADMAP.md` §20.10.

`ci/checks/gameplay-floor.toml` re-stamped by the script (GENERATED, not
hand-edited): `floor = 4`, `projection = "proj:7cefde0fb56d9f3e"`, same
ensemble, `runs = 6`, `p_hide = 0.393038` unchanged (N did not change so the
`LOWER BOUND` line does not either).

`ci/figures.toml`'s `corpus-floor-n6-coverage92` flipped `quotable = false` ->
`true`, `claim` moved `"7 of 92 …"` -> `"4 of 92 …"`, and `artefact_claim`
re-pointed at the new projection stamp (`projection = "proj:7cefde0fb56d9f3e"`)
rather than a bare number, per the brief — that is what let the previous
re-measure survive undetected.

Swept for the caveat this makes untrue: `docs/PARITY-ROADMAP.md` §19.1's table
note and §20.9's `STALE PROJECTION` blockquote each got a short follow-up
pointing at §20.10 rather than being rewritten (the superseded readings are
kept, per this project's convention). `docs/WORKLIST.md`'s server-track item 2
marked its follow-on DONE. Left `docs/PARITY.md` §4.1 alone — it is the older
88-scenario, `twin-homog-20260809`/`gpfloors-homog6-20260809` history and
already self-caveated ("quote whichever you like *with its ensemble*"); it
does not claim to be the standing figure and none of the brief's named greps
(`STALE PROJECTION`, `b385528330fee646`, `1e154ef8765b7525`, `floor = 7`) hit
it as a live claim. `ci/checks/gameplay-baseline.toml` untouched, per the
brief. `ci/checks/gameplay-unstable.toml`'s own `proj:b385528330fee646`
mentions are its own historical evidence comments (paired-control table from
`d6112fd9`) and are correct as written — not touched.

**What was NOT done and does not need to be:** no `## N.` heading was added to
`docs/DETERMINISM.md`, only a `### 20.10` under the existing `## 20.` in
`docs/PARITY-ROADMAP.md`, so no new `docs/claims/` file — `ci/check-doc-claims.py`
only adjudicates `## N.` headings. `ci/checks/gameplay-baseline.toml` untouched
(no verdict converts here). No re-record, no vanilla reference regenerated.

## Killed as a null

None — this item had one clean answer and the instrument gave it.

## Next

Nothing queued by this branch. The floor is quotable and stamped; the next
gameplay-corpus item is whatever `docs/WORKLIST.md`'s `### Next, in order`
says next.
