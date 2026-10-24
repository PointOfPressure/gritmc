# Cold start — branch `vacuous-green-audit`

---

### COLD START — branch `vacuous-green-audit` off `1d109abcf`, 2026-09-06

**One sweep of every check in `ci/` for the one defect this repo names as its
central class.** Five instances turned up on 2026-09-05, each found by a lane
looking for something else; the argument for doing it deliberately is that
finding them one at a time costs an agent each. Remote dir `~/rustmc-audit`.
Not merged; the orchestrator gates.

## The census, and what it is derived from

`ci/vacuous-green-census.py` reads the source and emits
`ci/checks/vacuous-green.toml`. Regenerate with `--write`; `--json` for the
whole thing including per-script call sites.

```
322  ci/*.py and ci/*.sh scanned
 94  declare a verdict (a non-zero exit whose adjacent message says RED /
     REFUSED / FAIL / UNREACHABLE — the thing a gate reads)
 18  of those are invoked by nothing in ci/ tools/ crates/ hooks/ swarm/
      plugins/ or the repo root
  8  exit-0 paths in 7 scripts whose own message says the check could not run
  0  dead exit handlers under `set -e`   (4 before this branch)
 24  invoked verdict scripts with no visible floor on their own population
      — a CANDIDATE list, not adjudicated, deliberately NOT gated
```

Two shapes are decidable from source and both are derived. Two are not, and
are carried as hand-adjudicated `DECLARED` entries with their evidence instead
of being guessed at: a failure path gated behind a data field that disables it
(the `provenance = "derived"` instance), and a fixture that cannot produce
what the check looks for.

## The instrument that keeps it honest

`ci/check-vacuous-green.py`, wired into `ci/t0.sh`, reds on: a new instance the
tree does not declare; a declaration left standing after its instance was
repaired; a committed census gone stale; a `cmd; VAR=$?` under `set -e`, which
has its own arm and its own sentence because it is never declarable; a
ratcheted count that rose; **and a census that has stopped finding anything.**

That last one is the reason the file has floors at all. Every comparison the
guard makes is a set comparison over the census's population, and an empty
population satisfies all of them. `SCAN_FLOOR = 280`, `VERDICT_FLOOR = 80`.

`DECLARED` is one entry per line with the reason in the comment block above it
— the `DORMANT_GENERATORS` shape from `ci/check-generator-arms.sh` — so two
lanes adding entries touch different lines.

`ci/check-vacuous-green-falsify.sh`: 9 cases, 41 assertions, ~96 s. Every case
asserts GREEN before the injection, asserts the red names **that** injection by
its own sentence, and asserts GREEN again after removal. Exit codes are not
enough: two falsifiers on this project passed green on 2026-09-05, one by
mutating an empty dict and one by reading a failed build as a caught mutation,
and both would have satisfied `[ "$rc" -ne 0 ]`. Cases 3 and 4 separate a
generator that runs and finds nothing (exit 1, the floor message) from one that
cannot run at all (exit 2, REFUSED), because a guard that answers both with the
same word cannot tell a shrunken tree from a broken tool.

## The strongest instrument, and the number it produced

Every one of `ci/t0.sh`'s **82** check invocations was run in two trees — a
worktree with history, and an rsync of the same tree made the way
`ci/t0-remote.sh` ships one (`--exclude target --exclude oracle-work
--exclude .git --exclude 'data/vanilla'`) — with `ssh`, `cargo`, `java` and
`javap` stubbed to exit 127 so "needs a toolchain" is a distinct answer.
Exactly **one** pair disagrees:

```
ci/figure-provenance.py --check   worktree  exit 1  56 re-derived, 56 not in their provenance
                                  gate dir  exit 0  SKIPPED — not inside a git repository
```

So `main` is red on 56 of 56 figures and no gate has ever been able to see it.
Repair is `origin/figure-provenance-gate` d80ffd9df and is not on main.

The A = B rows are worth as much as the one that differs: they say the other 81
behave the same in a gate directory as in a worktree, which is the property
nobody had checked.

## What was repaired here

* **`ci/check-cited-audits.py`** — the same defect at the same caller
  (`ci/t0.sh:116`), unrepaired. Lifted `figure-provenance-gate`'s idiom:
  `--snapshot` runs the real check where history exists and stamps the verdict
  into `.cited-audits-check`, bound to a sha256 of the ledger it was computed
  from; `ci/t0-remote.sh` stamps at the sync, the last line in the pipeline
  that can still see a commit; a gate replays it; a tree with neither
  repository nor stamp exits **2**, not 0. Second arm in the same file: an
  **empty `cited` table exited 0**.
* **Four dead exit handlers under `set -e`** — `ci/t0.sh:254`, `:300`, `:934`
  and `ci/loot-golden.sh:150`. All three T0 ones had a `case` arm distinguishing
  REFUSED from RED, and `ci/handoff.sh` reads the word.

## Nulls and hazards — do not re-run these

* **The census scanned its own output as evidence.** `ci/checks/*.toml` is
  data; a name in a manifest is a declaration, never a call. The first
  `--write` put `script = "ci/decoded-kind-reach.py"` into the census, the next
  run credited it as a call site, and `uninvoked` went 18 -> **0** with nothing
  in the tree having changed. Do not widen the scan back over `ci/checks/`.
* **Heredoc bodies are not the enclosing file's code.** `ci/handoff.sh:232` is
  `set -uo pipefail` and its five `VAR=$?` captures are correct; the
  `set -euo pipefail` at 1124 is inside `<<REMOTE_EOF`. Crediting it reported
  the whole gate pipeline as broken. `ci/check-vacuous-green-falsify.sh` reads
  as holding the instance it plants for the same reason.
* **A bare TOML key after a `[[table]]` header belongs to that table**, and a
  second key of that name inside `[meta]` is a parse error. The candidate list
  read back as absent and printed "0 scripts show no floor" over 23.
* **Never `run_guard | grep -q`.** `grep -q` exits at the first match, the
  producer takes SIGPIPE, and under `set -o pipefail` the pipeline reports 141
  — a passing assertion read as a failure. Cost case 2 its first run.
* **170 of the 322 scripts are not checks and calling them "uninvoked" would
  bury the real ones.** `ci/surface-golden.sh` regenerates golden vectors and
  is *meant* to be run by hand. The discriminator is whether the script emits a
  verdict on a non-zero exit. It is a proxy, it is stated in the module
  docstring, and the judgement about which side of it a script falls on lives
  in a reviewed `DECLARED` line rather than in a regex.
* **`no_population_floor` is not gated and should not be.** It is a static
  proxy that has not been adjudicated one entry at a time; ratcheting it would
  produce reds a lane can only clear by raising a number.

## The single next item, and the result that closes it

**Wire the four orphaned falsifiers, or retire them.**
`ci/batching-selftests.sh`, `ci/chat-commands-falsify.sh`,
`ci/movement-falsify.sh` and `ci/falsify-ruined-portal.py` are each a
red-when-broken proof for something that ships and each runs nowhere — the
shape `ci-gate-stage-falsifiability` found in `ci/handoff-verdict-selftest.sh`,
whose own header records it reporting 0 passed / 17 failed on unmodified main,
found by somebody running it by hand. Each needs its runtime measured first,
which is why they are declared here rather than wired. Closes when
`uninvoked_verdict_scripts` reaches 14 with the four moved into
`ci/checks/required-checks.toml` with a `wired_in`, or when each carries a
recorded reason for being retired instead.

**Second item, not mine:** land `origin/figure-provenance-gate`. Main is red
behind that skip on 56 of 56 figures and every gate reads exit 0.
