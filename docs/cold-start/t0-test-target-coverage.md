# Cold start — branch `t0-test-target-coverage`

---

### COLD START — branch `t0-test-target-coverage` off `911c2e04`, 2026-08-16

**The item was `check-test-targets: 110 targets declared, 22 run in T0, 88 do
not`, unowned for weeks, with nobody having established what the 88 meant. It
is two different things and neither is what the line says.**

Remote dir `~/rustmc-t0targets` on the Threadripper. No gate run, no corpus
lock taken, no box directory shared.

#### The partition, measured

    T0        22   ci/t0.sh: `--lib --bins`, plus three named selector lines
    T1        88   ci/t1.sh: bare `cargo test --workspace --no-fail-fast`,
                   which selects all 110 — so these 88 are its exclusive half
    T2         0   no cargo targets at all
    nowhere    0

**T2 runs no `cargo test` and cannot.** `ci/t2-wire.sh`, `ci/t2-gameplay.sh`,
`ci/t2-gameplay-subset.sh`, `ci/t2-gameplay-selfcheck.sh`, `ci/t2-memory.sh`,
`ci/t2-regiondiff.sh`, `ci/t2-vanilla-loads-ours.sh` and
`ci/t3-determinism.sh` were grepped on 2026-08-16 and run none: T2 drives
already-built binaries against recorded vanilla traces. So "covered by T2" is
not a thing a cargo target can be, and `ci/check-test-targets.py` deliberately
has no `t2` value for `runs_in` — a bucket cannot claim it.

**So the reported hole of 88 was a reporting defect.** Every one of the 88 runs
in T1, which `ci/handoff.sh` stage 5 runs on every merge. The line named a hole
of 88 where the measured hole is 0, which is how a check teaches its readers to
discount it.

#### "T1 runs it" against "T1 declares it" — the distinction, measured

The partition above is by **selector**, and a selector reaching a target is not
the same as the target being able to fail. The whole workspace suite was run in
`~/rustmc-t0targets` to separate them: 110 result blocks (98 binaries + 12
doctests), which **confirms the model** — cargo's bare selector really does
reach all 110, so `88 first run in T1` is measured and not parsed.

    1,738 passed, 2 failed, 8 ignored
    `skipping: no vanilla`  0     nothing skipped for want of a fixture
    zero-assertion targets  20    executed, reported ok, cannot fail

There is no feature flag and no untaken path in this workspace: no target is
`#[cfg(feature)]`-gated out of the run, and the fixture-skip count is 0, so
every one of the 110 was really executed. What separates declared from covered
here is emptiness, not conditionality:

* **9 doctest targets** hold no executable example (8 entirely vacuous, plus
  `rustmc_worldgen` whose only example is ```` ```ignore ````). Already recorded
  in the `doctests` bucket; confirmed unchanged.
* **2 integration targets** are `#[ignore]`d instruments — `dig_throughput`
  and `probe_sulfur_gradient`. Already recorded.
* **8 lib/bin unittest targets were not recorded anywhere and are the finding**:
  `anvil-probe`, `essentials-core`, `goldengen`, `permissions`, `region-differ`,
  `rustmc-api`, `rustmc-server` (the bin), `worldgen-dump`. All eight are in the
  `t0-unit` bucket — **T0 declares 22 targets and 14 of them can go red.** The
  bucket's `1455 assertions` figure is exactly what hid it: an aggregate over
  eighteen targets cannot show that eight contributed nothing.

Not a defect to fix by deleting anything — a crate is under no obligation to
carry unit tests, and `rustmc-server`'s bin is a thin `main` over a lib carrying
588. What it means is that `min_binaries = 106` and `min_tests = 1437` are both
floors an empty target satisfies.

**Left as prose on purpose, with the next item named.** The counts exist only
once the suite has run, so T0 compiles nothing and cannot see them, and a field
in `test-targets.toml` that nothing verifies would be the identical defect this
branch just repaired. The instrument is `ci/t1-allowlist.py`, which already
parses every `test result:` line for `min_binaries`/`min_tests`; the missing
piece is a per-target zero-assertion set in `ci/checks/t1-known-red.toml`
compared exactly, red when it grows and red when it shrinks. It needs its own
falsifier and that file has none, so it is a separate item.

#### Nothing was wired and nothing was retired

Both counts are **0**, and that is the measured answer rather than an omission:
the partition found no orphan to wire and no target that any stage fails to
reach, so a retirement would have had to be manufactured. Retiring
`probe_sulfur_gradient` was considered and rejected — `retired` in this ledger
means *no stage selects it*, and a file under `tests/` is always selected by
T1's bare selector, so the only way to retire one is to delete it. Its 250
lines of module doc are the record of the sulfur-gradient investigation and the
`#[ignore]` carries an explicit reason. Deleting a test to make a count go green
is the worst available outcome; the count stays at 0 and the file stays.

#### The two failures are the two standing reds, unmoved

`carved_chunks_match_vanilla` and `surface_chunks_match_vanilla`, both
`106 of 7077888 blocks differ (99.998502% exact)` at
`golden_carver_chunk.rs:410` and `golden_surface_chunk.rs:862` — byte-identical
to the signature `ci/checks/t1-known-red.toml` declares and to
`docs/AGENT-BRIEF.md`. No third failure, and nothing this branch touched moved
either. **No revived test failed, because nothing needed reviving.**

#### The real defect, which is the opposite shape

`ci/check-test-targets.py` read `ci/t0.sh`, recomputed its argv and refused if
the manifest disagreed. **It had never opened `ci/t1.sh`.** The 88 targets
marked `runs_in = "t1"` — twelve buckets, every golden vector and every
integration test in the workspace — were held up by twelve prose `reason`
fields and by no line of code. Narrowing `ci/t1.sh` to `--lib --bins` takes all
88 out of every gate in the project, and every check in the tree stays green
while the summary line prints exactly what it printed anyway.

That is this project's central defect class — a verdict whose green cannot be
turned red — inside the ledger written to close it, eight days after it was
written. `ci/checks/test-targets.toml` carries the full account.

#### What changed

* `ci/t1.sh`'s two `cargo test` lines carry the selector sentinel. The sentinel
  is now **stage-named** (`T0's` / `T1's`), so the commonest way a second stage
  gets wired — copying the first stage's line and its comment together — is a
  refusal rather than a silent misattribution.
* The scanner reads **logical** lines, joining `\`-continuations, and strips a
  leading run of `VAR=value` assignments. Both were holes rather than
  conveniences: `RUSTMC_FIXTURES=optional cargo test --workspace` on one line
  did not match `startswith("cargo test")`, so it would have carried no marker,
  raised no refusal and contributed no targets — a whole suite the ledger could
  not see. The tree never exercised it only because `ci/t1.sh` happens to write
  its env prefix on a continued line.
* `RUNS NOWHERE` reddens on a declared target no stage's selector reaches.
  `RETIRED BUT RUN` reddens on the reverse. `never` is renamed `retired`,
  because a value that reads as a fact about the world invites nobody to own
  it.
* The summary line reports the partition and names every retired target on its
  own line, so a retirement costs its author a line in every gate log until
  somebody deletes the target.

#### Red-when-broken evidence

`ci/check-test-targets-falsify.py`, **16 cases, 1.3 s, no build, no jar, no
box**, in `ci/t0.sh`. A real four-target cargo workspace under `mktemp -d` —
lib, bin in a second crate, integration test, doctest, chosen to span both
2026-08 defects — with the real checker copied in.

The case the item turns on is **`orphan-runs-nowhere`**: a target added to the
tree, given a bucket and a written reason, and selected by no stage. Green
before this branch; exit 1 naming the target after it. Its partner
`orphan-retired-is-green` asserts the repair is *accepted*, because a rule with
no green form only teaches people to route round it — and because the only
other way to close it would be to delete a test, which is the worst available
outcome.

#### Killed as a null — do not re-run this

**The invocation ring is deliberately not checked.** `runs_in = "t1"` now means
two things and the checker verifies one: that `ci/t1.sh`'s argv selects the
target, not that anything invokes `ci/t1.sh`. The chain is `ci/handoff.sh`
stage 5 → `ci/t1-gate.sh` → `ci/t1.sh`, one line.

Two measured reasons, both checked rather than assumed:

* deleting stage 5 is **not silent**. `ci/handoff.sh:489` is `set -euo
  pipefail` and `T1_RC` is assigned only inside stage 5, so stage 6's
  `[ "$T1_RC" -eq 0 ]` is an unbound-variable fatal. The failure mode is a dead
  gate, not a green one.
* the available check is worse than the gap. Shell invocation cannot be
  established by substring — `ci/handoff.sh` names `ci/t1.sh` twice in comments
  and executes it never — so any such check strips comments heuristically and
  is biased toward a **false green**, defending against a fault that has never
  occurred and would be loud if it did.

If a stage is ever silently un-invoked, that reasoning is wrong and the check
becomes worth writing. Nothing else about it needs re-deriving.
