# Cold start — branch `gameplay-baseline-lockins`

Append-only. A new `### COLD START` entry per turn on this branch; nothing
above is edited.

---

### COLD START — branch `gameplay-baseline-lockins`, off `main` (`40883b00`), 2026-08-11/12

**Item: `ci/checks/gameplay-baseline.toml` still recorded `diverge` for three
scenarios the corpus had been reprinting as `BETTER` since the 2026-08-11
23:19 gate.** Worktree `~/grit/work-lockin`. No `RUSTMC_REMOTE_DIR`, no
`ci/t0-remote.sh`, no corpus run taken — the item was closable from six gate
logs already on disk in `~/rustmc-private/gates/`, per the brief, and stayed
that way.

**Per-scenario count across the six named logs** (`gate-sulfur-cinnabar-r19-
20260811-143633`, `gate-gameplay-611-close-returns-grid-20260811-164923`,
`gate-corpus-stateid-decoder-20260811-184803`, `gate-corpus-floor-restamp-
20260811-203147`, `gate-corpus-vanilla-coverage-20260811-215953`,
`gate-gameplay-block-interactions-20260811-231959`, oldest to newest):

* `container-click-pickup-all-gathers-matching-stacks` — **match in all six.**
  No gate in the set ever recorded diverge. **Locked in.**
* `closing-a-crafting-table-returns-the-grid-contents` — diverge in the
  oldest two (14:36, 16:49), match in the newest four (18:48, 20:31, 21:59,
  23:19). The two-diverge/four-match split is the correct before/after pair
  across whatever landed on main between 16:49 and 18:48, not a live
  disagreement among tips that had it. **Locked in**, citing the four.
* `a-fence-gate-opened-from-behind-turns-to-face-you` — diverge in the
  oldest five (14:36 through 21:59), match in **only** the newest one
  (23:19, `gameplay-block-interactions`'s own hand port). That is the
  correct before/after shape the brief flagged in advance ("check whether an
  earlier gate saw it diverge — it should have"), but it is **one gate**, not
  two independent ones: no other tip in the set has this hand port yet to
  corroborate it. **Not locked in.** This is the finding to carry forward —
  re-check after a second, independent gate lands on a tip that includes
  `gameplay-block-interactions`'s fence-gate port (merge to main, or another
  branch rebasing past it), and lock it in then with that second citation.

Each lock-in is its own commit naming the logs. The fence-gate line in
`ci/checks/gameplay-baseline.toml` is untouched; no other line in that file
was touched.

**The `WORSE` advisory path is live, checked two ways without a corpus
run.** The mechanism is `Baseline::check` in
`tools/oracle-runner/src/compare.rs` (`worse`/`better` returned from
comparing each outcome's verdict against the baselined `expect`, `bad.extend
(worse)` in `main.rs`, printed as `REGRESSED` and turning a green gate red).

1. The existing unit test `a_baseline_refuses_a_regression_and_notices_an_
   improvement` (`compare.rs`) already asserts a baselined `match` regressing
   to `Absent` lands in `worse`. It is one of oracle-runner's 94 `#[test]`s,
   all 94 of which passed 0-failed in the most recent gate on disk
   (`gate-gameplay-block-interactions-20260811-231959.log`, "running 94
   tests" / "94 passed; 0 failed"), and it re-ran green locally just now
   (`cargo test -q -p oracle-runner --bins
   a_baseline_refuses_a_regression_and_notices_an_improvement`, this box,
   2026-08-12).
2. Injected a throwaway fake-regression test in the same file (a baselined
   `match` scenario reported back as `Diverge`) and it caught it —
   `worse.len() == 1`, message `"...was match, is now diverge..."`. Ran green
   locally, then reverted (`git checkout -- tools/oracle-runner/src/
   compare.rs`); not part of any commit on this branch. `Cargo.lock` also
   picked up an unrelated `grit-plugin-api` version bump from the local build
   (0.2.1 -> 0.3.0, this box's `../plugins` checkout) and was reverted the
   same way, untouched by any commit here.

**Next item on this branch: none — closed.** The one open thread is the
fence-gate second-corroboration re-check above, which belongs to whichever
branch next gates a tip carrying `gameplay-block-interactions`'s fence-gate
port, not to a fresh corpus run taken for its own sake.
