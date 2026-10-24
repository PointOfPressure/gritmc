# CI — gates, merge queue, flake policy, spend caps

The swarm's throughput is set by verdict latency, not coding speed. CI is the
factory; these are its specs. Gate ids (G1–G5) match ORACLE.md §7.

## Where a run goes

The laptop is a working machine, not a build farm. **One quick command runs
locally; anything heavy or repeated runs on the Threadripper**, over
`ssh threadripper`.

| run | where |
|---|---|
| `cargo build -p <crate>`, one crate's unit tests, `javap`, greps | laptop |
| `cargo test --workspace --no-fail-fast`, `cargo clippy --all-targets` | Threadripper |
| golden-vector regeneration (`ci/*-golden.sh` over a real chunk) | Threadripper |
| benchmarks, region-diff, determinism double-runs | Threadripper |
| **two of anything at once** | Threadripper |

Never launch a second heavy job locally while the first is still running:
four stacked `cargo test --workspace` runs took the laptop's load average to
13.8 and made it unusable for its owner.

Two practical notes. `cargo` is not on the PATH of a non-login SSH shell, so
prefix with `export PATH="$HOME/.cargo/bin:$PATH"`. And the Threadripper's
checkout only has what git tracks — `data/vanilla/` is gitignored, so rsync it
across before running a gate that reads it.

## Gate tiers

| Tier | Gate | Runs | Wall-clock target | Blocking? |
|---|---|---|---|---|
| T0 | `bash -n` sweep (floored) + the `ci/*.py` self-tests + **the test-target manifest** (`ci/check-test-targets.py`) + `cargo check`/clippy/fmt `--all-targets` + `cargo test --workspace --lib --bins` + the same four in the sibling `../plugins` contract | every agent push | < 1 min | yes (agent-local) |
| T1 | **G1** unit + golden-vector tests, whole workspace (`ci/t1-gate.sh`) | **every merge, since 2026-08-05** | **~19 min measured** (the `~11 min` here was a target, not a reading — see the stage budget below) | yes (merge) |
| T2 | **G2** wire checks mapped to touched modules (in-repo harness: `ci/t2-wire.sh` calibrates vs vanilla, `ci/t2-selfwire.sh` grades rustmc once w1-server-skeleton lands; `ci/t2-regiondiff.sh` proves the differ) | every change | < 5 min | yes |
| T3 | **G2 full** all Class A+B shards + **G3** region-diff r3 + **G4** determinism double-run | merge train only | 15–30 min | yes (merge) |
| T4 | **G3 full** region-diff r18 (ratchet) + **G5** bench smoke vs Paper | nightly / pre-tag | hours | tag gate |

### The persistence gates, and the stage that finally runs them

The scripts behind the save path — the ones that measure it against real
vanilla, the one that signals a running server with a player still connected,
and the one that measures the measurement — are **stage 7 of
`ci/handoff.sh`**, as of 2026-09-05. Before that none of them was invoked by
anything automatic: `ci/t2-vanilla-loads-ours.sh` had been in the tree since
August and had never once been run by a gate, and this section said so and
stopped there.

| gate | what it proves | control |
|---|---|---|
| `ci/t2-vanilla-loads-ours.sh` | a **region file** we wrote is one vanilla reads | `--negative-control` flips one byte at the midpoint of the first-poked chunk's compressed stream; arm A must go red, arm C must stay green, and vanilla must complain |
| `ci/t2-vanilla-opens-our-world.sh` | a **world directory** we wrote is one vanilla opens — it reports our seed against a decoy in `server.properties`, and leaves `ServerBrands` as `rustmc,vanilla` | `--negative-control` deletes `world_gen_settings.dat` and requires the decoy |
| `ci/t2-player-survives-a-restart.sh` | a player walks, logs out, the server restarts, and they rejoin where they left, with `players/data` agreeing | `--negative-control` wipes the world between the boots and requires the spawn |
| `ci/t2-sigterm-saves.sh` | a **clean stop saves a player who never disconnected** — the probe holds its socket open, the autosave is pushed to an hour, and SIGTERM is what writes the file | `--negative-control` sends SIGKILL, which no program can catch; the file must be absent and the exit status 137 |
| `ci/anvil-byte-identity-falsify.sh` | the 754-chunk byte-identity golden can go red — three shipped defects reinserted one at a time | green control at both ends |

`ci/t2-persistence.sh` is the stage driver. It takes the corpus lock once
around the whole set (role `gate` when handoff runs it), asserts on each
script's own GREEN sentinel rather than on its exit status, and runs every
control — a control arm exits 0 when it *works*, so the status cannot tell a
fired control from a crashed one and the sentinel is the only thing that can.

**The driver has its own red-when-broken evidence**, in T0 and needing no
gate: `ci/t2-persistence-falsify.sh`, 17 assertions over a lab of fake gates.
It requires a RED from a gate that prints no sentinel, from one that prints
its sentinel and then exits non-zero, and — the case a status-reading driver
cannot see at all — **from a control that did not fire**, which exits 0 and
prints the gate's ordinary green line. Its case 0 is a lab where everything
behaves and must go GREEN, or the rest would pass on a driver that reddens
unconditionally.

**It is conditional, and the skip is a third state.**
`ci/persistence-applies.py` reads `ci/checks/persistence-paths.toml` and runs
the stage only when the branch touches a declared path. A branch that does not
gets `t2p=n/a` in the verdict line, which is a different string from
`t2p=0/1`; `t2p=skipped` means an earlier stage was red. The three are never
interchangeable, which is the distinction `t1=99` did not have until
2026-08-18 and which cost 113 wrong RED verdicts in one morning.

Ports: 25937, 25939, 26787 and 26791, one per gate. The falsifier rebuilds
`rustmc-world` four times. Run the stage by hand after touching `encode_chunk`,
`decode_chunk`, `ChunkColumn`'s fields, `rustmc_world::level`,
`rustmc_world::playerdat`, `rustmc_server::levelstore`,
`rustmc_server::autosave` or `rustmc_server::shutdown` — or just let the merge
gate do it, which is the point.

### What a merge gate actually costs, by stage — 2026-08-18

The wall-clock column above is a set of targets. Until today no gate log
anywhere carried a timestamp on a stage boundary, so every statement this
project has made about gate cost was reconstructed by differencing other files.
This is the first budget assembled from measurements, and it is assembled from
**three independent sources** rather than one, which is why it is worth writing
down before `gate-stage-timing` lands and makes it a routine readout.

| stage | cost | share | source |
|---|---:|---:|---|
| 1 prepare | 1 s | | `STAGE-TIME` markers, N=1 |
| 2 rsync | 2 s | | `STAGE-TIME` markers, N=1 |
| 3 tables | 3 s | | `STAGE-TIME` markers, N=1 |
| 4 **T0** | **149 s** -> **~166 s** -> **~191 s** | 4% | `STAGE-TIME` markers, N=1; +17 s and +25 s added 2026-08-18, both timed directly |
| 5 **T1** | **~19 min** | 30% | residual, corroborated — see below |
| 6 **T2 corpus, in-lock** | **43.3 min** | 66% | refcache probe, wall − lock-wait |
| **total** | **65.2 min** | | measured mean of 127 GREEN gates |

**Where each number comes from, because no single instrument gives the whole
budget.**

* Stages 1-4 come from the only gate log in existence that carries
  `STAGE-TIME` lines — `gate-gate-stage-timing-20260818-000839.log`, the
  `gate-stage-timing` branch gating itself before the 00:15 power-off killed
  it. **N = 1**, and it is quoted as such.
* Stage 6 comes from the reference-cache probe's uncached pass, taken as
  `wall − (the log's own "ACQUIRED as gate after waiting Ns")`, because that
  probe ran beside the live chain and its raw wall time measures the queue.
* **The 65.2 min total is measured independently of both**, over 127 GREEN
  gate logs (`ci/gate-rate.py --cost`), as log mtime minus the stamp in the
  filename.
* **T0 grew by 17 s on 2026-08-18 and the arrow is in the table rather than a
  silent overwrite**, because the 149 s reading is N=1 and correcting it by
  arithmetic would destroy the only direct measurement anyone has. Timed
  individually on this box:

  ```text
    ci/figure-provenance.py --check     15.21 s   <- 87% of the addition
    ci/check-figures-falsify.py          2.69 s   (was ~0.9 s at 13 cases; 40 now)
    ci/check-figures.py                  0.58 s
    ci/check-cited-audits-falsify.py     0.33 s
    ci/check-cited-audits.py             0.07 s
  ```

  **Almost all of it is one check and it is `git log -S` bound**, re-deriving
  the recovered provenance of 56 figures against their own documents. If T0
  ever becomes the stage worth optimising — it is 4% of a gate, so not yet —
  that is the line to cache, and caching it means keying on the doc blob rather
  than skipping the check. None of these need a build, a jar or the lock, which
  is why they sit in T0 rather than later.
* Stage 5 is then the residual, 19.3 min — **and it is corroborated rather
  than merely inferred.** The five `T1-GATE RED` gates take 21.3 min to reach
  their verdict; subtracting the 2.6 min of stages 1-4 leaves 18.7 min of T1
  before failure, which is a *lower* bound on a full T1 run. Two routes,
  ~19 min, agreeing to within the precision either supports.

**Two things follow, and the second is the one that matters.**

1. **The `~11 min` T1 target in the table above was understated by ~8 min**,
   and has been corrected in place. A wall-clock target that is quoted for long
   enough becomes a figure people plan against.
2. **T2 is two thirds of every gate, and the reference cache halves it.**
   `docs/REFERENCE-CACHE-FLAG.md` measures the cached in-lock hold at 21.6 min
   against 43.3 uncached. Applied to this budget the mean gate goes
   **65.2 → 43.5 min, which is 1.50x on every gate the chain runs** — larger
   than any other throughput change currently proposed, and it is one line.

**What this budget cannot yet tell you** is where inside T1 and T2 the time
goes, or how the shares move on a branch that touches only worldgen. That needs
per-stage timing on every gate rather than on one, which is exactly what
`gate-stage-timing` adds and why it is the pinned branch.

**A gameplay corpus score is never quoted without its floor.** `ci/t2-gameplay.sh`
prints, under every `CORPUS declared X/N`, how many of those scenarios two
independently recorded **vanilla** runs answer differently — 9 of 88 at N = 6
on 2026-08-09 — together with the resolving power that bounds it. The floor is
an annotation on the denominator and is never subtracted from the numerator;
nothing in the declared-unstable list can change a verdict, and
`ci/corpus-floor-falsify.sh` case 11 asserts that this gate does not so much as
read it. Measure it with `ci/corpus-floor.sh <vanilla.json> ...` over any two
or more recordings on the box; the protocol is `docs/PARITY-ROADMAP.md` §13 and
ORACLE rule 28.

**Two gates are per-change rather than per-tier, and the reason is worth
stating: they need a DUMP, so they cannot live in a one-minute tier, and they
answer a question no tier asks.**

* **`ci/be-channel-identical.sh`** — the block-entity side channel's closing
  condition (`docs/DETERMINISM.md` §42.5, §43.2). One binary generates the same
  window twice, with `RUSTMC_RECORD_BLOCK_ENTITIES` on and off, and the block
  states must be identical: the channel carries data OUT of generation and stays
  sound only while nothing reads it back in. ~2 min at r=4, no corpus lock, no
  vanilla boot. **Run it after touching anything under
  `crates/rustmc-worldgen/src/` that records or emits a block entity.** It
  carries its own positive control — every recorded class must be absent from
  the plain arm and present in the recording arm — and reports **VACUOUS** on a
  window that exercises none of them, because two arms that both wrote nothing
  agree perfectly.
* **`ci/jigsaw-stream-cost.sh`**, **`ci/jigsaw-stream-bediff.sh`** and
  **`ci/jigsaw-stream-villages.sh`** — what threading the jigsaw driver's piece
  stream costs and what it buys (`docs/DETERMINISM.md` §44). Three scripts and
  not one, deliberately:
  * **`-cost`** takes the BLOCK-STATE figure on the quotable seed-12345 r=16
    window, off one binary, with `RUSTMC_JIGSAW_STREAM` unset / `1` / `2`. Its
    primary reading is ours-against-ours, so it needs no reference and no floor.
    It **refuses** a threaded arm that spent zero draws, because a pass with no
    draws in it produces the same zero as a pass with no effect.
  * **`-bediff`** takes the BLOCK-ENTITY reading, and it is separate because it
    needs `RUSTMC_RECORD_BLOCK_ENTITIES=1` while every published block-state
    figure on that window was taken without it. §42.5's identity gate is what
    makes the split sound, and this script re-checks it.
  * **`-villages`** is the arm the seed-12345 window cannot exercise: that
    window reaches no village start and no chunk with two starts of one
    structure, so neither the `feature_pool_element` desync nor the
    `LongOpenHashSet` start order is tested there. It runs at seed 20260722 and
    **opens no vanilla reference at all**, because that window carries no
    quotable exactness figure and a script that opens no reference cannot
    publish a level by accident.

  Run `-cost` and `-bediff` after touching `structure::place::place_piece` or
  either jigsaw call site. None of the three takes the corpus lock.
* **`ci/be-payload-r16-12345.sh`** — the payload reading behind
  `ow-r16-12345-be-payload-20260817`. Dumps the quotable r=16 window with the
  channel on, reads the reference's own `HARVEST-ORDER` stamp to set the dump
  order (ORACLE rule 29, and it refuses an unstamped reference), and reports
  `bediff` vanilla first. ~4 min. It quotes the payload half and **refuses to
  publish the union-based exactness**, whose denominator mixes presence with
  payload.

**Both refuse on a stale generated table rather than working around it, and one
of them found that the obvious place to copy tables from is stale.** A tree
seeded with `data/vanilla` from the canonical checkout at
`/home/sayer/grit/repo` got a `block_props.json` from 2026-08-03 with **zero**
occurrences of `randomlyTicking` against 32,367 in a freshly generated one, and
`worldgen-dump` refused with the column named and `ci/gen-gameplay-tables.sh`
in the message — which is a check working, since the alternative is silently
treating every state as non-random-ticking. `repo` is the checkout the gate
drives, not a table source; seed from `~/rustmc/data/vanilla` per
`docs/AGENT-BRIEF.md` and regenerate. **A golden that fails in a tree whose
branch touched none of it is this, not a defect** — four `rustmc-server`
command goldens failed exactly this way on 2026-08-17 and went 588/588 green
after regeneration, with no code change.

**And the half of the channel's condition neither of them can see is in T0.**
On/off identity is blind to a producer that spends a draw to build its payload,
because the draw is spent in both arms and the two worlds move together while
both are wrong. That half is the draw-count test in
`feature::monster_room`, and `ci/be-channel-falsify.sh` injects both defects and
asserts which instrument catches which — including that the identity gate stays
green on the one it cannot see.

**The whole loop is `./ci/handoff.sh <branch>`**: it prepares the gate
directory, then runs T0, `gen-gameplay-tables.sh`, T1 and the T2 gameplay
corpus in it, requires `T0 GREEN`, `T1-GATE GREEN`, `T2-GAMEPLAY GREEN` and
zero `skipping: no vanilla` lines, and only then
fast-forwards and pushes. **The `skipping: no vanilla` count in that list is
not load-bearing and never was** — it is 0 on every run, for the reasons in
`docs/PARITY-ROADMAP.md` §73, and `ci/t1-gate.sh` now refuses before running
instead of adjudicating a log afterwards. It is left in the list because
`ci/handoff.sh` still computes it over its own shell-stage output, where the
grep does work.
`RUSTMC_REMOTE_DIR` names the gate directory once
and every stage reaches it through the same variable — on 2026-08-03 the
stages ran on two different trees and a clean branch was reported 12/55 with
11 regressed. See `ci/provenance.sh` for the stamps that now make that a
refusal instead of a number.

### What the gate's own commands can still be green over

Audited command by command on 2026-08-07 by breaking a tree in exactly one way
and running the real command on it. Four holes were closed in that pass (the
rsync mtime one in `ci/t0-remote.sh`, the `#[ignore]` one in
`ci/t1-allowlist.py`, the unlinted `../plugins` contract and the unfloored
parse sweep in `ci/t0.sh`, and the table derivation in
`ci/gen-gameplay-tables.sh` + `ci/t2-gameplay.sh`). These were measured and are
**not** closed:

* **`cargo check --all-targets` does not compile doctests, and T0's
  `--lib --bins` does not run them.** A doctest that does not compile takes T0
  to `T0 GREEN`, measured. T1 catches it — `cargo test --workspace` runs
  doctests, and the adjudicator reddens on the count mismatch even though its
  `---- name stdout ----` regex cannot read a doctest's name, which contains
  spaces. So the hole is T0-only, and T0 is what an agent runs before a push.
  **Sized on 2026-08-10 and it is much smaller than it reads**
  (`docs/DETERMINISM.md` §28): the 12 doctest targets hold **three** executable
  doc examples in the whole workspace — `rustmc-proto`, `rustmc-data`,
  `rustmc-api`, one each — two more are ` ```ignore `, and **eight of the twelve
  targets are entirely vacuous**, reporting `0 passed; 0 failed; 0 ignored`. A
  target with no example in it cannot have one that fails to compile, so the
  live exposure is three examples in three crates. The count is now enumerated
  in `ci/checks/test-targets.toml`, so a thirteenth lib crate cannot join this
  hole without a red gate. Not closed — sized.
* **T0 tests the sibling `../plugins` workspace more thoroughly than this one.**
  Measured 2026-08-10. `ci/t0.sh`'s second `cargo test -q --workspace` runs
  inside the `cd "$PLUGINS_DIR"` subshell and is **bare**, so it reaches that
  workspace's integration tests and doctests; this workspace's line is
  `--lib --bins` and reaches neither. The contract crate is better gated on
  every agent push than the server that depends on it. It is also reachable only
  if everything above it passes under `set -e`, so the plugin contract sits
  behind this workspace's whole test suite.
* **`--all-features` is a no-op today and is not in any command.** No crate in
  the workspace declares `[features]` and no source uses `cfg(feature = …)`,
  measured the same day. The first feature added is outside every gate until
  somebody adds the flag.
* **The gate's laptop-side driver is supplied by the branch under test.**
  `ci/handoff.sh` reads `ci/t0-remote.sh` — which decides what is shipped and
  what is excluded — from disk *after* `git checkout "$BRANCH"`. Marked LATENT
  in that file on 2026-08-06 and still open; the fix is to rsync from a
  throwaway detached worktree.
* ~~**`ci/handoff.sh`'s final verdict reads an errored test as false.**~~
  **CLOSED 2026-08-18** by `ci-handoff-terminal-states`. The analysis was
  right and the consequence was worse than "narrow": confirmed directly,
  `T0_GREEN=$(grep -c ... /nonexistent)` leaves the variable EMPTY, every
  `[ "" -eq 0 ]` in the chain exits 2, the `if` reads the whole condition as
  false, and the gate **falls through to the merge with no sentinel assertion
  having fired at all** — a green certified by no evidence. The narrowness
  argument was also wrong in the other direction: if the log is merely
  truncated rather than absent the counters read a clean 0 and the gate
  reddens a healthy branch instead, which is how it was first found.
  `ci/handoff.sh` now refuses to start when the log cannot be written (INFRA,
  exit 4, because the instrument failed and not the branch), and asserts
  immediately before the counters that the log still contains the header
  **this run** wrote into it — identity rather than existence, because the
  provenance census re-creates a deleted log and an existence test passes over
  the recreated one. Forced both ways by `ci/handoff-state-falsify.sh`
  (`unwritable gate log`) and `ci/handoff-merge-tail-falsify.sh`
  (`log eaten mid-gate is INFRA`).
* **A `data/vanilla` path that is not one quoted literal derives nothing.**
  `concat!("data/vanilla/", "beds.json")` is invisible to both the generator
  and the corpus preflight. A blanket "any unmatched mention is an error"
  guard was tried and rejected: it fires on the version-pinned jar extracts
  (`hash_order.json`, `structure/`), which have no generator.
* **`prov_gen_digest` still does not cover `GAME_VERSION`** — marked LATENT in
  `ci/provenance.sh` on 2026-08-06, unfired only because the version has not
  moved.

**A gate directory is not a view of another tree.** `oracle-work/`'s vanilla
references are shared read-only and linked in individually; `oracle-work/
gameplay/` and `data/vanilla/` are per-tree, because the second is generated
from the tree's own dumpers and is therefore branch-dependent. Run
`./ci/gen-gameplay-tables.sh` once in any new directory.

**T1 is in the gate as of 2026-08-05, and the hole it closed was the biggest
one in this table.** T1 exits 101 on two honestly-red golden vectors
(`carved_chunks_match_vanilla`, `surface_chunks_match_vanilla` — one defect in
the surface rule, reported twice), and a permanently-red stage gets switched
off within a day, so it had never been wired in. T0 selects `--lib --bins`,
which reaches a fraction of the workspace's test binaries; T1 reaches all of
them. **Every golden vector and every integration test under `tests/` were
outside every merge gate for the project's whole history.** The count moves
as crates are added, so it is not quoted here — `ci/t1-gate.sh` prints the
measured figure on every run, and `min_binaries` in
`ci/checks/t1-known-red.toml` is the floor that is actually enforced. That is
the same family as `--lib` never reaching `tools/oracle-runner`, one level
up: not a gate that could not fail, but a gate nobody ran.

`ci/t1-gate.sh` runs the suite and `ci/t1-allowlist.py` adjudicates it against
`ci/checks/t1-known-red.toml`, **by signature and not by name** — the entry
records the count, the denominator, the y bands, the confusion table and the
seed/chunk of the first example, so 106 mismatched blocks becoming 10,600 is
as red as a third failure appearing. A name-only allowlist would let a known
failure get quietly worse, which is the stale-baseline defect that let a
regression pass as GREEN on 2026-08-04. A declared red that *stops* failing is
also red: a stale caveat list reads as authoritative and is worse than none.

**`min_binaries` is a floor on binaries, not on tests, and that was a hole.**
A binary in which every test is annotated away still reports
`test result: ok. 0 passed; 0 failed; N ignored`, so the binary count cannot
see a test stop running. Measured 2026-08-07 on a replica of this workspace: a
failing unit test adjudicated `T1-GATE RED — a new red`; the only edit was
`#[ignore]` on that one test; the same adjudicator over the new log printed
`T1-GATE GREEN` with the binary count unchanged. `min_tests` (a floor on tests
passed) and `max_ignored` (a ceiling on tests skipped) close it, and both are
required — an absent declaration would default to a bound that could not fail.
All three are printed by `./ci/t1-allowlist.py --emit <log>`.

It runs **after** `gen-gameplay-tables.sh` and **before** the corpus. After,
because several T1 binaries read `data/vanilla` tables that script
regenerates under the branch's own provenance (seven at last count, across
`rustmc-server` and `rustmc-worldgen` — not quoted as a fixed number because
a new golden vector changes it), and a gate directory seeds that directory
from a shared one with `cp -rn`. Before the corpus, because 676 s against ~50
minutes means a golden-vector regression surfaces in eleven minutes instead of
an hour.

`ci/t1-gate.sh` used to refuse on any `skipping: no vanilla` line in its own
log, on the argument that T1's output stays in a file on the box and so never
passes through `handoff.sh`'s identical grep — "the guard has to live where the
output does". **The output does not exist.** `cargo test` never prints a
passing test's stdout or stderr and a test that returns early passes, so the
line is never written, and the count is 0 whether or not the gate directory
still has its `oracle-work` references. The failure that paragraph describes —
"would otherwise have run the worldgen integration tests as no-ops and reported
green" — is what was happening while it was written.

It now checks the fixtures **before** running the suite, from the filesystem,
and exits 2 naming each missing one. Nothing is inferred from a log, so no
pattern can be too narrow. `docs/PARITY-ROADMAP.md` §73 has the measurement:
with the datagen extract removed, T1 reported the same 111 binaries and 1749 of
1758 tests passing, in 122 s instead of 1521 s.

**`--lib --bins`, and why the `--bins` is load-bearing: a gate that cannot
fail is not a gate.** T0 ran `cargo test --workspace --lib` for weeks.
`tools/oracle-runner` is a **binary** crate, so `--lib` matched none of its
targets — nineteen tests had never run in any gate, and one was failing. Every
"T0 GREEN" reported in that period was, on that crate, meaningless. Found and
fixed 2026-08-03.

The general rule, which has now cost three separate items here: **ask of any
green result what would have had to happen for it to come back red.** If there
is no answer, the instrument was measured and not the code. When a crate or a
target is added, check the gate's selector still reaches it.

- **Test-impact map** (module → PlayTest shard list, from the Phase-0
  `playtest-manifest.json`): an agent porting beacons never waits on phantom
  suites. The map is generated, not hand-maintained.
- **Sharding:** N rustmc instances on N ports (Threadripper), suites
  round-robined. Port allocation = 25600 + shard index; every agent worktree
  gets its own port range, Minecom-style.
- **Worldgen ratchet:** G3-full is up-only from the inherited Minecom number
  (99.381792% overworld **r18, seed 20260708** at doc time — inherit whatever
  it is at port time). A change that drops it reverts, no debate — the Minecom
  rule verbatim. **This number is inherited and not measured here**, and this
  project has never taken an r18 measurement — see `docs/ORACLE.md` §3. It is
  not comparable to any r=7 or r=16 figure in `docs/PARITY.md`, and the ratchet
  cannot actually be run until an r18 window exists.

### What `ci/handoff.sh` can say, and what each one asks you to do

Eight words, one exit code each, and the pairing is enforced —
`ci/checks/handoff-terminals.toml` declares it and `ci/check-handoff-terminals.py`
fails T0 if the code drifts from it. **The vocabulary is prefix-free as a
checked property.** The verdict this loop got most wrong was
`MERGED-NOT-PUSHED`, which `grep -E '^MERGED'` matches, so the state meaning
"the queue is about to wedge" was namable as the state meaning "everything
worked" — 2 of the 32 REDs in this loop's record arrived as another branch's
MERGED.

| verdict | exit | was the branch judged? | what to do |
|---|---:|---|---|
| `MERGED` | 0 | green, and origin has it | nothing. Drop it from the queue. |
| `ALREADY-MERGED` | 0 | green, by an earlier run | nothing. Idempotent re-run, not a fresh merge. |
| `GREEN` | 0 | green, and NOT merged | `--dry-run` only. The caller merges. |
| `RED` | 1 | yes, and it failed | read the diff. Re-running fails again. |
| `REFUSED` | 2 | no | fix the local state named. Cheap. |
| `NOT-FF` | 3 | no | rebase onto main and re-gate. |
| `INFRA` | 4 | **no — nothing was measured** | fix the infrastructure and re-run. The branch is unjudged, not bad. |
| `UNPUSHED` | 5 | green, and merged in LOCAL main | **push main.** Nothing else can be gated until main and origin/main agree. |

`UNPUSHED` is the one that is about the queue rather than the branch, and it
exists because of 2026-08-16. A gate went green, merged, and the push timed out.
`BEHIND` is measured against LOCAL `main` — correct, since a fast-forward is a
local operation — so one unpushed merge made every branch behind it read
`N behind main — not a fast-forward, rebase it` and refuse, on a rebase none of
them needed. One transient network failure, 37 branches reported as needing
hands. The disagreement is now detected at entry as well as at exit, repaired
where it can be (`git push origin main` sends all of `main` regardless, so
pushing early changes *when* those commits leave and never *what* leaves), and
named where it cannot be — a genuine divergence is not something a gate script
may resolve.

Under `--dry-run` the entry check reports and never repairs: `ci/lanes.sh`
drives every lane that way and does its own merge under its own sequencer, and
a lane's local `main` is legitimately ahead of `origin/main` between one lane
merging and the next one pushing.

## Merge queue (bors-style; the serial core of the swarm)

- Agents never push to `main`. They push branches + open a queue entry.
- The queue rebases each entry onto current `main`, runs T1–T3, merges on
  green, next entry. One integration stream — this is the Amdahl floor and
  it's intentional: 20 agents' throughput dies from merge chaos, not from
  queue latency.
- **Interface-change entries jump the queue but freeze it**: nothing else
  merges until the orchestrator lands the interface bump and the affected
  agents rebase (see CRATES.md frozen-interface rule).
- Queue verdicts are machine-readable; agents self-serve their failure logs
  (tail of the failing shard only — token discipline).

### The queue ORDER is state, and it was being lost silently — 2026-08-18

The chain is serial at ~80 min a gate, so where a branch sits in the queue is
worth roughly `position x 80 min` to that branch. The ordering lives outside
this repo, in the orchestration scripts (`preflight.sh`, `chain-keeper.sh`,
`supervisor.sh`), and it had a race that cost the throughput pin about eight
hours without logging anything at all. Written down here because the scripts
are untracked and a correction that lives only in the file it corrected does
not travel.

**The mechanism.** `preflight.sh` triages every queued branch with the
tree-only checks and writes one row per branch to `preflight.tsv`;
`chain-keeper.sh` orders the queue from that file, guarded by
`[ -s preflight.tsv ]`, and pins a hand-written `PRIORITY` list first.
`supervisor.sh` re-runs `preflight.sh` when it goes stale. Both the re-run and
the queue rebuild fire on a tick conditioned on "no chain is running", so they
race by construction — and `preflight.sh` truncated its output in place and
appended across its whole ~2 min sweep. Two ways to lose:

* **empty** — `[ -s ]` false, so the *entire* ordering block was skipped, pin
  included, and the queue kept raw commit-date order. Nothing logged it;
* **partial** — `[ -s ]` true over a prefix of the branches, so the ordering
  half-applied. Worse, because it looks like it worked.

**It fired.** `07:15:24 supervisor: preflight was 426m stale — re-running`,
`07:15:25 keeper: relaunching over 46 branch(es)`, `preflight.tsv` mtime 07:17
against `queue7.txt` mtime 07:15. `gate-stage-timing` — first in `PRIORITY`,
GREEN in preflight, one commit — came out at **position 7 of 46**.

**Two independent repairs, because one of them is the general lesson.**

1. `preflight.sh` writes a temp file and renames it. A rename within one
   filesystem is atomic, so a reader sees the whole previous sweep or the whole
   new one and never a prefix of either. An empty sweep now **keeps** the
   previous file and exits 3, rather than publishing nothing over something.
2. `chain-keeper.sh`'s `PRIORITY` pin no longer sits inside the
   `[ -s preflight.tsv ]` conditional. It is a hand-written list of branch
   names and needs nothing preflight produces; nesting it made an unrelated
   file's availability able to cancel it. A skipped *ordering* now says so in
   `gate-chain.log` instead of being silent.

**The generalisation worth keeping**: a consumer guarded by `[ -s file ]` and a
producer that truncates in place are a race whose failure mode is *silence*,
and silence in a queue reads as a scheduling decision rather than as a fault.
Publish by rename; and never make a hand-written override conditional on data
it does not depend on.

Evidence: `preflight-falsify.sh` case 6, which reconstructs the shipped defect
by patch and asserts that it destroys the file while the repair keeps it —
without that half, "the repair keeps the file" is satisfied by a script that
never writes at all. 13 assertions / 6 cases. `chain-falsify.sh` 33/33 after
the keeper change.

## Zero-flake policy (load-bearing at swarm scale)

At 20 agents × ~200 runs/day, a 1% flake rate = 40 false reds daily and the
gate stops meaning anything — worse, agents will "fix" phantom failures and
corrupt the tree. Rules, inherited from Minecom's hard-won discipline:

1. **A check that fails intermittently is a bug** — in the test or the
   product. "Environmental flakiness" is a hypothesis to test on an idle
   machine, never a verdict to ship on.
2. **Second flake = quarantine**: the check moves to a quarantine shard
   (non-blocking, still runs, still logged) and a fix task is queued. The
   quarantine shard's size is public and trends to zero before any tag.
3. **Fix patterns, in order of preference:** state gates / conserved
   quantities / bounded-budget invariant loops ("armed ⇒ detonates within N
   ticks"). **Widening tolerances is forbidden** — it converts a signal into
   a lie.
4. **Cross-thread assertion rule** (bought with the sulfur-fuse diagnosis):
   never assert an exact instantaneous value of state owned by another
   thread/task; assert invariants that hold under a free-running scheduler.
5. **Determinism double-runs (G4)** catch the nastiest class before it
   flakes: byte-identical output from identical seeds, under load, twice.

## Seeds & reproducibility

- Every suite run logs its seed set; failures re-run with the same seeds
  before being called real.
- The canonical worldgen seed (20260708) and bench scenarios are constants in
  CI config, not folklore.
- Logs are durable artifacts (Minecom rule: never only in /tmp) — CI keeps
  the last N failing logs per shard, agents get the tail.

## Spend caps & token discipline

- **Budget (user-set 2026-07-22): £40/WEEK hard maximum, target ~£5/day.** Per-key OpenRouter daily limit £5–8, prepaid top-ups of ~£10 at a time (prepaid = physically cannot overspend). Orchestrator reports burn rate after wave 1 before any scale-up — a
  runaway retry loop costs a bad day, not the summer. Orchestrator gets a
  daily spend report next to the green-bar report.
- **Stable prompt prefixes** (system prompt + frozen spec docs first,
  changing diff/task last) — cache-hit input is ~10× cheaper; at swarm scale
  this is the difference between £200 and £2,000 for the same build.
- Agents read **sections, not files**; failure logs arrive pre-tailed; the
  full transcript never enters a worker's context.
- Escalation is cheaper than repetition: a worker that fails a task twice
  stops and files an escalation (SWARM-PLAYBOOK.md ladder) instead of burning
  a third attempt.

## Release pipeline (the 48h-SLA machinery, VISION.md)

- Poller (10-min cron, laptop or scheduled cloud agent) watches
  `version_manifest_v2.json`.
- On new **patch** version: unattended run = fetch jar → `goldengen` +
  extractor regen → T1–T4 against new data → swarm ports red deltas →
  T1–T4 green → RC published + status page updated. Human phone-ping only on
  stall. Contractual clock: 48h; typical: same-day.
- On new **major**: pipeline still runs (produces the red-delta inventory =
  the public tracker's content), but the target date is set by a human on
  the next weekend pulse. No fixed-hour promise on majors, ever.
- Status page is generated from CI state — it is not hand-updated (a
  hand-updated status page dies in September).

## Running the whole-world diff on the Threadripper

The heavy runs — `worldgen-dump full` over an r7 square, and the region
diffs — belong on the Threadripper (`ssh threadripper`, 24 cores). Two
things bite, both because the inputs are **gitignored**, so `git pull` on
that machine updates the code and silently leaves the data behind:

```sh
rsync -a data/vanilla/            threadripper:~/rustmc/data/vanilla/
rsync -a oracle-work/vanilla-r7 oracle-work/vanilla-s2-r7 \
                                  threadripper:~/rustmc/oracle-work/
```

- `data/vanilla/` is the extracted registry. A stale copy fails loudly —
  `minecraft:air has no centerFaces` — which is the good case. Sync it
  whenever a goldengen harness gains a field.
- `oracle-work/` holds the vanilla worlds the diff compares against. Never
  commit either; they are 25 MB and 11 MB a world.

`cargo` is not on a non-login SSH shell's `PATH`; prefix
`export PATH="$HOME/.cargo/bin:$PATH"`.

Worth doing for its own sake: the Threadripper reproduced seed 31415926's
2,187 mismatches exactly, which is a determinism check across two machines
and two CPUs that no single-machine rerun can give.

## Finding a measurement window with structures in it

Both r7 windows are ocean-ish, so between them vanilla places one shipwreck,
one buried treasure and one ocean ruin — and nothing else from the other
nineteen structure sets. A parity number taken there says almost nothing
about structures.

Vanilla will say where its own structures are. `ci/vanilla-server.sh` already
has a console:

```sh
D=oracle-work/gen-locate-25580
./ci/vanilla-server.sh start $D 25580 <seed> default
for st in village_plains stronghold pillager_outpost ruined_portal; do
    ./ci/vanilla-server.sh console $D "locate structure minecraft:$st"
    sleep 2
done
grep -oE 'The nearest minecraft:[a-z_]+ is at \[[-0-9]+, [~0-9-]+, [-0-9]+\]' $D/server.log
```

At seed 20260722 that gives, in blocks:

```text
  village_plains     -848, -528      chunk (-53, -33)
  stronghold         -368, 1568
  pillager_outpost   -320, 32
  ruined_portal      -528, -464
  desert_pyramid    -1424, -3056
```

Then generate an oracle world centred there and diff as usual. This is far
cheaper than generating squares until one happens to contain something, and
it makes coverage a choice rather than an accident — which is the same
argument that got the second seed added.
