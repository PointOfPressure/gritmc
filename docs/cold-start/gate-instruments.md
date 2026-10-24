# Cold start — branch `gate-instruments`

---

### COLD START — branch `gate-instruments` off `main` = `1a31e0a56`, 2026-09-05

**The gate's own instruments: the projection pin, the declared binary count, and
the provenance rule that never runs where it is enforced.** Worktree
`/home/sayer/grit/work-gate2`, remote dir `~/rustmc-gate2`. No server code
touched — one comment in `tools/oracle-runner/src/compare.rs` and nothing else
under `crates/` or `tools/`. Not merged; the orchestrator gates.

#### What moved

    rule 18 in a gate dir       green by not looking  ->  3 reds, same as with .git
    figure-provenance --check   56/56 failing         ->  0/56 failing
    orphaned provenances        3 (48d1efb, 00ab0bde, 917ebc43)  ->  0
    corpus floor                4/92 @ proj:7cefde0..  ->  5/108 @ proj:1277e43..
    ci/loot-golden.sh per setup  4 invocations         ->  1
    readiness-falsify race      main lost 1 gate in 5  ->  picked from stack-ci

#### The three items

1. **The projection pin.** `ci/checks/gameplay-floor.toml` pinned
   `proj:7cefde0fb56d9f3e`; main computes `proj:1277e43a7e157183`. Stale on the
   denominator too — 92 declared against 108 scenarios — and its ensemble's six
   recordings lived in `~/rustmc-cov2`, gone. Re-derived from recordings already
   on the box: **5/108**, N=6, 15 pairs, one recorder, p_hide 39.3%.
   `docs/PARITY-ROADMAP.md` §108. `compare.rs:156-159`'s claim that "no change to
   the recorder can move the projection identity again" is corrected with the
   bisect: **10 of the 14** commits touching a projection source between the old
   stamp and main moved it, `trace.rs` 6 of them, because `trace.rs` is a
   projection source AND holds the recorder's decoder (`trace::observe` is
   called from the `trace::observe` call in `tools/oracle-runner/src/client.rs::pump` (`:1383`) and from nothing else).

2. **The declared binary count — REFUSED, and this is not a deferral.**
   `ci/checks/t1-positive-controls.toml` and `ci/check-t1-controls.py` **do not
   exist on main**; `grep -rn total_binaries ci/ docs/` at `1a31e0a56` returns
   nothing. Both live only on `origin/stack-ci`, 3050 commits, 293 ahead. The
   repair is one hunk when that stack lands and is written out in
   `docs/FINDINGS.md`: derive `total_binaries` from `cargo_test_targets()`
   (`ci/check-t1-controls.py:176` already computes it) and keep the
   classification arithmetic at line 154 as the thing a merge obliges a lane to
   measure. Creating the file on main would collide with that landing.

3. **The provenance rule.** `ci/t0-remote.sh` rsyncs `--exclude .git`, so rule 18
   fell back to shape and `--check` printed SKIPPED. Repaired by shipping the
   ANSWER, not the database: `ci/figure-provenance.py --snapshot` stamps
   `/.figure-provenance` (172 rows, gitignored) in the worktree that has history
   and `ci/t0-remote.sh` writes it immediately before the rsync — the `/.rp-commit`
   idiom. `.git` was the alternative and is 165 MB. Rule 18 now REFUSES on an
   absent or stale stamp instead of degrading.

#### Killed as nulls — do not re-run these

* **Shipping `.git` to the gate.** Measured: `/home/sayer/grit/repo/.git` is
  165 MB, and it would give every gate directory a HEAD disagreeing with its own
  working tree, which is the property `--exclude .git` was chosen for.
* **Retiring the three orphaned figures.** All three re-derive from the
  documents with agreeing witnesses (`ab462e11f`, `689fb52fc`, `f3d04d6b8`), so
  none of them is a figure that cannot be re-derived.
* **`git cherry-pick ad0f4287` alone.** It applies cleanly and leaves the tree
  `LIVENESS-PROOFS RED — UNREGISTERED ci/readiness-falsify.sh:201`. The pair
  `42305f106` + `07be1ecf5` is the unit; `ci/readiness-falsify.sh` is now
  byte-identical to `origin/stack-ci`'s.
* **Filing `a-daylight-detector-inverts-when-right-clicked` in
  `ci/checks/gameplay-unstable.toml`.** `reference` and `boundary` have opposite
  fixes and a first difference at 0 ms fits both; `ci/corpus-floor.py adjudicate`
  refuses a claim filed under the wrong one. It is that scenario's owner's call.

#### The single next item

**`ci/liveness-proofs.py --discover` misses a wait-on-a-log written as a
multi-line `if ... then break; fi`** and finds the identical wait written as
`grep -q ... && break` — 1 discovered, then 0, same semantics both times, which
is what `07be1ecf5` exploits rather than repairs. So `6/6 discovered` is 6 of the
loops whose FORMATTING the discoverer matches, not 6 of the loops in `ci/*.sh`.
Closing it needs the discoverer widened AND a fixture pair recorded from a real
`rustmc-server` boot for `ci/readiness-falsify.sh`'s own boot wait, because the
registry's rule is that a proof ships with a log it rejects and typing one from
the pattern is the defect the registry exists to refuse. The result that closes
it: `--discover` finds the same loop in both spellings, and `--check` is `7/7`
with `0 pinned unsound`.
