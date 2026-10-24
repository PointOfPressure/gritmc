# Cold start — branch `t0-assertion-floor`

---

### COLD START — branch `t0-assertion-floor` off `186d6f17`, 2026-08-16

**The item was the prose `t0-test-target-coverage` left behind: 20 of the 110
cargo targets execute zero assertions and cannot fail, 9 of them inside T0's
own bucket.** It is now a check, a ratchet, and 18 fewer targets.

Remote dirs `~/rustmc-assertfloor` (baseline suite) and `~/rustmc-assertfloor2`
(the fixed tree) on the Threadripper. No gate run, no corpus lock taken, no box
directory shared.

#### The 20, re-derived rather than copied — and the previous count was wrong twice

Derived two independent ways that agree target for target: a **static** walk of
the module tree each target roots (`ci/check-test-targets.py`), and a
**measured** whole-workspace suite against `186d6f17` (110 result blocks,
`EXIT=101` at the two standing reds, `skipping: no vanilla` = 0). The static
model agrees with all 110 measured blocks in both directions.

| group | n | targets |
|---|---|---|
| lib/bin unittest binaries with no `#[test]` | 9 | `anvil-probe`(bin) `essentials-core`(lib) `goldengen`(bin) `permissions`(lib) `region-differ`(bin) `rustmc-api`(lib) `rustmc-server`(bin) `worldedit-core`(lib) `worldgen-dump`(bin) |
| doctest targets with no example rustdoc runs | 9 | `essentials-core` `permissions` `rustmc-behavior` `rustmc-entity` `rustmc-server` `rustmc-world` `rustmc-worldgen` `worldedit-core` `worldgen-assembly` |
| `#[ignore]`d instruments | 2 | `dig_throughput` `probe_sulfur_gradient` |

**`186d6f17`'s bullet list said EIGHT lib/bin while its own total said 20.** The
ninth is `worldedit-core::lib::worldedit_core`. All nine are in `t0-unit`, so
the headline figure was wrong in the direction that flattered the gate:

    T0 declared 22 targets and 13 could go red, not 14.

The doctest group was recorded as eight in the ledger; it is nine —
`rustmc-worldgen`'s only fence is ` ```ignore ` (`trace_switch.rs:52`), so it
reports `0 passed, 1 ignored` rather than `0/0/0` and fell out of a count
keyed on the vacuous shape.

#### Outcomes: 18 made to assert, 2 declared compile-only

    T0: 22 declared, 13 could go red  ->  22 declared, 22 can go red
    T1: 110 declared, 108 can go red (the 2 instruments)

**Made to assert (18).** Four bins had pure decoders or argv parsers worth a
test outright; `rustmc-server`'s bin had its flag reader extracted; the three
plugin stubs and `rustmc-api` assert type-level and architectural properties;
all nine doctest targets gained a runnable example chosen to document a
property that has actually been got wrong in this problem space (vanilla's
`Direction` ordinal order, the 32-bit `imul` in `Mth.getSeed`, `>> 4` against a
truncating divide at a negative chunk border, an absolute data path against a
`cargo test` binary's own cwd).

**Declared compile-only (2)**, `[[compile_only]]` in
`ci/checks/test-targets.toml` with the argument written out per target:
`dig_throughput` (drives the whole production dig path; a gate keeps it
type-checking) and `probe_sulfur_gradient` (`#[ignore]`d for a
`sulfur_mismatch.json` a whole-world diff produces; **not retired** — the sulfur
residual is alive at 38.0% of §11 and 63.5% of §12's remainder, and the file's
own header carries the 2026-07-27 finding, so deleting it would delete a
recorded result to make a count go green).

**Nothing was retired and nothing was deleted.** Retirement was considered for
`probe_sulfur_gradient` and refused on the above.

#### TWO PROPERTIES DID NOT HOLD ONCE THEIR TARGET STARTED ASSERTING

**1. `region-differ` could not tell a malformed window from an absent one.**
`parse_bbox`/`parse_ybox` were `filter_map(|s| s.parse().ok())` behind a
`v.len() == 4` test returning a bare `Option`. The operands are positional, so

    region-differ census <dir> --bbox 1 2 3 --match minecraft:sulfur

read `--match` as the fourth integer, dropped it, saw three integers and
answered `None` — and `cmd_census` reads `None` as *no window* and runs over
every chunk in the directory. That is the unbounded census `docs/AGENT-BRIEF.md`
records as having pushed 35.6M blocks down an ssh pipe and taken the box off the
network, reachable from a plausible argv. It is also worse than a hang: a census
whose window silently widens is a number that answers a different question from
the one in the commit message, and rule 3 says name the region a count was taken
over. Now `Ok(None)` / `Ok(Some)` / `Err`, with the `Err` a print-and-exit at the
call site. Six tests.

**2. `rustmc-api`'s "plugins depend on rustmc-api ONLY" could not be obeyed.**
Stated as `ENFORCED` in the API header and repeated as "enforced workspace rule"
in all three first-party plugins. Nothing enforced it — and nothing *could*:
`Server::send_message` takes `EntityId`, `Event::BlockPlace` carries `BlockPos`
and `BlockStateId`, and none of the three were reachable through `rustmc-api`.
A plugin could satisfy the API only by breaking the rule the API states. Fixed
by re-export (not a mirrored type — a mirrored `EntityId` is the mistake the
workspace `Cargo.toml` records refusing for `grit-plugin-api`), and the three
plugins now assert their own `Cargo.toml` against the rule via `include_str!`,
with a negative control proving the reader sees a second dependency.

**And the same silent-default shape in `crates/rustmc-server/src/main.rs`:**
`--port`, `--view-distance`, `--radius` and `--autosave-seconds` each fell back
to the default on a value that did not parse. `ci/t2-gameplay.sh` sets
`--view-distance` and `--radius` on both sides *because* a differential trace
whose two sides load different amounts of world is not a comparison — so a
typo or an out-of-range value reverted our side to the default while the
harness's log recorded what it had asked for. Now a refusal.

#### The enforcement

`EXECUTES NO ASSERTION` — a declared target that a stage's argv selects, that
holds no runnable test or doc example, and that is not in `[[compile_only]]`.

The compile-only category's membership is **enforced, not described**: at least
one `#[test]` present and every one of them carrying `#[ignore]`. A target with
no test cannot qualify (that is the leftover case, and it is the nine); a doc
target cannot qualify in either direction (rustdoc does not compile an
`ignore`d fence, so there is nothing for the claim to be true of).

Ratchets in `[assertions]`, both granularities, because neither subsumes the
other — a target with 588 tests and a target with 1 both count as one target:

    min_red_capable_t0 = 22        min_red_capable_t1 = 108
    min_runnable_tests_t0 = 1622

Registered as three `gate-floor` figures; `ci/check-figures.py` demanded it
with `UNREGISTERED-FLOOR` on the first run, exactly as it did for `min_targets`
on 2026-08-10.

`ci/check-test-targets-falsify.py` 16 cases -> **40**. The two that decide
whether any of this is worth anything: `empty-target-not-admitted-by-compile-only`
(the category bent to absorb the leftover, refused on its criterion rather than
on judgement) and `ratchet-one-assertion-of-several` (a T0 target loses 1 of 2
tests — the target count does not move, so only the test-level floor sees it,
which is the 2026-08-07 `#[ignore]`-the-failing-test finding one level up).

#### Why the check is here and not where `186d6f17` predicted

That commit named `ci/t1-allowlist.py`, on the ground that per-target counts
only exist once a suite has run. They do — but a static derivation from the
sources cargo would build gives the same answer, and lands in **T0**, where an
agent meets it before the push rather than at merge.

The static model is not trusted. `./ci/check-test-targets.py --calibrate <log>`
compares it against a real suite log per target and refuses on any disagreement
in either direction; five falsification cases prove that comparison can fail.
It found a real bug in its own first draft: `rustmc-server`'s lib and bin both
build to `deps/rustmc_server-<hash>`, so a reader keyed on the binary stem
cannot tell 588 executed from 0 executed. The `Running` line's source path is
what separates them, and the falsifier's fixture now has a lib and a bin of the
same name so that case cannot regress.

#### NULLS — do not re-run these

* **A per-target zero-assertion set in `ci/checks/t1-known-red.toml`, adjudicated
  by `ci/t1-allowlist.py`** — the next step `186d6f17` named. Not done, and
  deliberately: it would duplicate the T0 refusal at merge time, one tier later,
  over the same fact, and `ci/t1-allowlist.py` still has no falsifier of its
  own. The measured-log half is `--calibrate`, which is one command over any T1
  log that already exists.
* **Retiring `probe_sulfur_gradient`** — considered and refused; see above.
* **A tautology test on the three plugin stubs.** `assert_eq!(Permissions.name(),
  "permissions")` is a constant compared against a copy of itself. What they
  assert instead is `Plugin::name`'s *documented shape* (lowercase machine name,
  no spaces) and their own manifests.

#### The declared blind spot

`ci/check-test-targets.py` counts **tests**, not assertions. A test that
executes and asserts nothing therefore reads as red-capable — `margin_probe`
and `probe_margin_context` are both env-gated, return early, report `1 passed`
and cannot fail. Named in `[assertions] blind_to` rather than rounded off.
Closing it needs a coverage instrument or a per-test mutation sweep, neither of
which is derivable from source.

#### THE NEXT ITEM

**Two targets in T1 execute a test that cannot fail, and the instrument that
found the other twenty is blind to them by construction.** The closing result
is a per-test check that an executed test's outcome can be moved — the same
question `ci/corpus-falsifiable.py` answers for gameplay scenarios by mutating
the subject side, asked of `#[test]` functions. `margin_probe` and
`probe_margin_context` are the two known instances and are the fixture; the
open question is whether a cheap form exists that does not need a mutation
build of the whole workspace.
