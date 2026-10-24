# Cold start — branch `ci-probe-stances`

---

### COLD START — branch `ci-probe-stances` off `3428600a`, 2026-08-13

**The briefed question has no answer because its premise is false.** The item
was "`ci/gen-gameplay-tables.sh` writes 0 `probeStances` while
`ci/checks/use-probe-dependent.toml` pins 12 — find the commit where 12 stopped
matching 0, and decide whether it is a generator regression or a stale pin."

It is neither, and there is no such commit.

* **The pin has never moved.** `stances = 12` entered the tree with the sweep
  itself, at `4f784669` (2026-08-12 10:25), and is still 12 at `3428600a`.
  `git log -S'stances = 12' -- ci/checks/` returns exactly two commits, both of
  which *introduce* the value: `4f784669` and `b75927fa` (the second allowlist).
  The only thing that has ever changed in that `[meta]` block is `unavailable`,
  `["full_inventory"]` -> `[]` at `b75927fa`, when `UseGolden.bindItemComponents`
  made the twelfth stance settable.
* **The generator has never regressed.** `UseGolden` writes `STANCES[1..]`, so a
  correct run emits eleven entries and `ci/check-use-probe.py` adds one for
  `base`. Run in a fresh tree today it emits exactly that: 11 + base = 12,
  `unavailable = []`, 17 of 184 rows probe-dependent and all 17 already allowed.

**The 0 is not a number the generator produced. It is the absence of a key in a
table the generator was never asked to write.**

#### The actual chain, every link measured

1. `docs/AGENT-BRIEF.md`'s HARD PRECONDITION snippet provisions a tree with an
   `ssh` that does **not** export `$HOME/.cargo/bin`. A non-login ssh to the box
   has PATH `/usr/local/sbin:…:/snap/bin` and no `cargo` — checked directly.
2. `ci/gen-gameplay-tables.sh` derives nineteen tables and runs their generators
   in `sort -u` order. The twelfth, `ci/dump-recipe-unlocks.sh:233`, calls
   `cargo build -q -p oracle-runner` and dies. The loop's
   `"$ROOT/$gen" || { …; exit 1; }` does its job and stops there — the exit
   status was never lost, which `ci-gentables-exit-status` established
   independently from the other end.
3. `data/vanilla/use_without_item.json` sorts **nineteenth of nineteen**, seven
   positions past the death. It is never regenerated, so the tree keeps the
   `cp -rn` copy of `~/rustmc/data/vanilla/use_without_item.json` — dated
   **2026-08-08**, four days older than the stance sweep, with **no
   `probeStances` key at all**.
4. `ci/check-use-probe.py:50` computes `len(doc.get("probeStances", [])) + 1` =
   **1** against a pin of 12, and `ci/t0.sh:229` exits 1 with *"the dumper swept
   1 stances"* — a statement about a dumper that had not run, made by a control
   that had measured nothing.

#### What it cost, which is the real payload

`ci/t0.sh` aborts at line 229, so **twelve stages had not executed in any freshly
provisioned tree since 2026-08-12** — among them the entire Rust build and test
suite. Run to completion for the first time (fresh `git archive` tree on the box,
cargo on PATH), all twelve are **green**:

| line | stage | what it says now |
|---|---|---|
| 249 | `ci/step-window-audit.py --selftest` | pass (silent) |
| 264 | `ci/corpus-surface.py --selftest` | pass (silent) |
| 280 | `ci/vanilla-reach-census.py --selftest` | pass (silent) |
| 305 | `ci/corpus-floor-falsify.sh` | `21 passed, 0 failed` |
| 335 | `ci/check-test-targets.py` | `107 targets declared, 22 run in T0, 85 do not` |
| 347 | `cargo fmt --check` | clean |
| 348 | `cargo clippy --all-targets -- -D warnings` | clean |
| 349 | `cargo check -q --all-targets` | clean |
| 378 | `cargo test --workspace --lib --bins` | 573 tests: 572 passed, 1 ignored; 795 tests: 795 passed |
| 409 | `cargo test -p rustmc-server --test session_joint` | 8 passed, 23.08 s |
| 433 | `cargo test -p rustmc-server --test plugin_storage --test plugin_tick_lock --test random_tick_pos` | 8 passed, 3.13 s |
| 481-494 | `../plugins`: `fmt`, `clippy`, `check`, `cargo test --workspace` | 95 passed |

Whole run: **29 test binaries, 1,658 passed, 0 failed, 2 ignored, `T0 GREEN`**.
The brief said "expect more reds and treat each as a finding". There are none.
Nothing was hiding behind line 229 — which is itself the finding, and it means
the two T0 cycles and the one wrong diagnosis this red cost were the entire
damage.

#### What landed here, and what deliberately did not

**Landed** (`e00264b2`): `ci/check-use-probe-falsify.sh`'s two `real-table`
controls now consult `prov_check` before running. Absent table — skipped, as
before. Present but not this tree's — still red, because a tree reading another
branch's golden tables must not print `T0 GREEN`, but red about the *tree* and
naming the repair rather than convicting the dumper. The guard carries its own
injection, `unstamped-table-refused`; an unfalsifiable guard inside a
falsification harness is the one way to make that file worse.

**Deliberately not landed — the PATH export.** The one-line fix to the
`AGENT-BRIEF` snippet is already written on `corpus-lock-fairness`
(`4e641442`, line 76) and that branch is first in the gate queue. Duplicating
it here would collide textually on a file every branch reads. **It is not on
`main`** — `git merge-base --is-ancestor origin/corpus-lock-fairness main` is
false — which is exactly why every tree still reds, and why
`ci-gentables-exit-status`'s note that the PATH half was "already fixed by
somebody else" is true of a branch and false of the tree everyone provisions
from. That branch merging is what actually closes this.

#### NULLS — do not re-run these

* **There is no commit where 12 became 0.** `git log -S` on the pin and on
  `probeStances`, both directions, both allowlists. The pin is original and
  unchanged. Stop looking for it.
* **The generator is not silently producing nothing.** It produces a correct
  table in ~14 s whenever it is reached. This is *not* another instance of the
  signature defect `ci-gentables-exit-status` chases; that branch's own
  reproduction and this one agree that the status propagates correctly and the
  generator that dies is named on stderr.
* **The stale local table in a long-lived worktree is a different red** —
  11 stances but `unavailable = ["full_inventory"]`, i.e. a table generated
  between `4f784669` and `b75927fa`. It reds on the `unavailable` drift, not on
  the stance count, and it is fixed by regenerating. Do not read it as the fresh
  tree's failure; they have different messages and different causes.

#### Next item, and the result that closes it

Merge `corpus-lock-fairness`. It is first in the queue and carries the PATH
export. The result that closes this: a tree provisioned by the `AGENT-BRIEF`
snippet **verbatim, with no hand-added `export PATH`**, runs
`ci/gen-gameplay-tables.sh` to `19 tables current` and `ci/t0.sh` to `T0 GREEN`.
Until then the guard added here is what stops the next agent spending a turn on
the wrong suspect.
