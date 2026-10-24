# Cold start — branch `gameplay-parity-scope`

Append-only. A new `### COLD START` entry per turn on this branch; nothing
above is edited.

---

### COLD START — branch `gameplay-parity-scope`, off `main` (`bba29ee2`), 2026-08-17

**Item: after the merge-conflict work on the gameplay corpus finished and the
queue went gate-bound, picked up actual gameplay parity — the corpus's own
numbers, not a re-litigated priority.** Worktree `~/grit/gw-parity`. No
`RUSTMC_REMOTE_DIR`, no `ci/t0-remote.sh`, no fresh corpus run taken — this
item closed from evidence already on disk in two other trees.

**Why this scenario, over the other ten declared divergences.** Checked every
`diverge`/`absent` row in `ci/checks/gameplay-baseline.toml` (108 total: 87
match, 15 diverge, 6 absent on `bba29ee2`) against `docs/PARITY-ROADMAP.md`
and `docs/cold-start/*.md` for an existing root-cause writeup, ranked by
mention count. The top two by mentions —
`a-player-that-leaves-the-ground-is-resynced` (11) and
`a-flower-pot-empties-when-right-clicked` (7) — are both about *measurement
stability* (`8/15`, `9/15` pair floors, "NEWLY UNSTABLE"), not a closeable
mechanism gap; `sand-falls-when-unsupported` is a whole missing entity
lifecycle (`FallingBlockEntity`, DROPPED deliberately, real feature-sized
work). `a-fence-gate-opened-from-behind-turns-to-face-you` was different: its
own cold start (`docs/cold-start/gameplay-baseline-lockins.md`,
2026-08-11/12) already named the exact condition for closing it — "re-check
after a second, independent gate lands on a tip that includes
`gameplay-block-interactions`'s fence-gate port" — and that condition had
quietly become true six days ago without anyone going back to check.

**THE CHECK.** `gameplay-block-interactions`'s `fence_gate()` port
(`crates/rustmc-server/src/useblock.rs`, commit `0915d909`) has been an
ancestor of `origin/main` since 2026-08-11. `git merge-base --is-ancestor
0915d909 <commit>` against two independent trees on this box, both far
downstream and unrelated to each other:

* `~/grit/work-falsev-sound`, commit `0615e789`, `oracle-work/gameplay/report.json`
* `~/grit/wt-servreach`, commit `e437ab99`, `oracle-work/gameplay/report.json`

Both report `"name": "a-fence-gate-opened-from-behind-turns-to-face-you",
"verdict": "match", "strict": "match"`, `4/4` reference and subject
observations, `missing_kinds: []`, `extra_kinds: []`. The observation payloads
agree with vanilla's own trace on the `state` field both before and after the
click (`facing=north,in_wall=false,open=true,powered=false`); the only
difference between the two `.json` traces was `move_entity_heartbeat` timing
noise on an unrelated observer entity, not this scenario's own field.

**WHAT CLOSED.** `ci/checks/gameplay-baseline.toml`: `diverge` -> `match` for
`a-fence-gate-opened-from-behind-turns-to-face-you`, cited in place. No source
change — the fix already existed; the ratchet had not caught up to it. Corpus
count: 87 match -> 88 match, 15 diverge -> 14 diverge, out of 108.

**Also swept:** every `ci/checks/gameplay/*.toml` file name in the tree —
none numbered 790 or above (790 itself is the highest; the brief's warning
about a stray item above it does not apply here, checked rather than assumed).

**NOT done, and why, so the next session does not re-derive it:**

* **The `random-ticks-turn-covered-grass-to-dirt` `absent` scenario** needs
  console + game rules support in `rustmc-server` at all
  (`tools/oracle-runner/src/main.rs:239` — no console, no game rules). Not
  scoped further this turn: it is a new subsystem, not a port, and estimating
  its cost honestly needs a read of every other `absent`/`diverge` scenario
  that a game-rule toggle would also unblock (`spawn-egg-makes-a-mob` and
  `a-dropped-item-goes-into-your-inventory` are declared-unstable for an
  unrelated reason and would NOT be unblocked by it — see
  `ci/checks/gameplay-unstable.toml`). Worth a dedicated turn with a
  worklist item of its own rather than a paragraph tacked onto this one.
* **The other nine declared divergences** are real gaps
  (`sand-falls-when-unsupported`'s missing `FallingBlockEntity`,
  `flint-and-steel-lights-a-fire`, the four redstone/piston/observer/repeater
  checks, `a-note-block-retunes-when-right-clicked`,
  `container-click-swap-exchanges-a-hotbar-slot`,
  `a-daylight-detector-inverts-when-right-clicked`) with no existing
  root-cause writeup found in `docs/PARITY-ROADMAP.md` or
  `docs/cold-start/*.md` under the scenario's own name. Each needs its own
  bytecode read before it is honest to estimate; none was started this turn.
