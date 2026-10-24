# Gate forensics — did the `rsync -a` freshness hole ever authorise a merge?

Written 2026-08-07. Evidence-only document: it settles one question about the
past and changes no code. The fix itself lives on `gate-command-coverage`
(`ca45d7a`) and is not this document's subject.

## THE ANSWER

**No. On the evidence that survives, the hole never fired on a gate run.**

Sixty-one gate runs have a log on disk that names a gate directory and a tip.
Fifty-seven of them are **provably incapable** of having shipped a back-dated
source file — not "no back-dated file was observed", but *no such file could
have existed*, from an argument over git's write behaviour that needs no
surviving build artefact. Four are **CANNOT TELL** and are named below; **one
merge** sits inside that bucket (`oracle-hardening-stack`, `bcc7ac1`, main
1,110 → 1,147, 2026-08-03 23:24). **Zero** runs are classified COMPILED NOTHING.

The reason is not luck and it is not the driver. It is a structural property of
`ci/handoff.sh` that nobody appears to have designed for:

> The gate directory's content **is** a copy of `$ROOT` taken by the previous
> run in that same directory. So any file that differs between `$ROOT` and the
> gate directory — which is exactly the set `rsync` transfers — must have been
> *written in `$ROOT` since that copy was taken*, and therefore carries an
> mtime later than the copy. The hole needs an mtime older than the gate
> directory's **build**, and the build always comes after the copy.

The one window in which that argument fails is a file written in `$ROOT`
between a run's stage-2 rsync and that same run's last compile — i.e. a
**second gate running concurrently, or a manual checkout, landing inside another
run's build window**. Three concurrent-run pairs exist in the record. All three
were checked individually and all three are clean, one of them by direct
artefact evidence rather than by argument.

So the hole was **real but latent**: a live mechanism sitting behind an
invariant that the merge loop happened to maintain, and that nothing in the
loop was asserting. That distinction is the deliverable, and it is the reason
the fix on `gate-command-coverage` is still worth having — the invariant is
undocumented, unenforced, and was broken three times in one afternoon by
concurrency without anyone noticing.

## THE HOLE, STATED PRECISELY

`ci/t0-remote.sh` (as on `origin/main` at `e3ca9ba`) ships the tree with

```
rsync -az --delete --exclude 'target' --exclude 'oracle-work' \
      --exclude '.git' --exclude 'data/vanilla' "$ROOT/" "$HOST:$DEST/"
```

`-a` implies `-t`, so a transferred file lands carrying the mtime it had on the
laptop. `target/` is excluded, so the gate directory keeps the previous run's
build cache and fingerprints. Cargo decides freshness by comparing a unit's
source mtimes against its fingerprint's mtime. A source that arrives **older**
than the last build is `Fresh`; `cargo check`, `cargo clippy -D warnings` and
`cargo test` then all report success on a file no compiler has read, and
`ci/t2-gameplay.sh` runs the previous tree's binary.

## THE DETECTION SIGNATURE

### First: the signature the question assumes does not exist here

A real compile emits `Compiling <crate>` and a non-trivial `Finished ... in Ns`.
**No gate log in this project contains either string, ever.** `ci/t0.sh` runs
`cargo clippy --all-targets -q`, `cargo check -q --all-targets` and
`cargo test -q --workspace --lib --bins`; `ci/t1.sh` runs `cargo test -q`;
`ci/t2-gameplay.sh` runs `cargo build -q`. `-q` suppresses cargo's status
output. Measured across all 108 files in `~/rustmc-private/gates/`:

```
grep -c '^ *Compiling '  ->  0 in every file
grep -c 'Finished'       ->  0 in every file
```

Worse, and measured rather than assumed (lab step below): the T0 transcript of
a run that compiled a whole workspace is **byte-identical** to the transcript of
a run that compiled nothing. Both are a sequence of `running N tests` /
`test result: ok.` blocks and the line `T0 GREEN`. A stale binary re-runs its
own cached tests and prints the same counts.

**A gate log alone cannot distinguish the two cases.** That is a finding, not a
limitation to be worked around: it means the log format this loop has kept for
its whole life records nothing about whether the gate compiled.

### Instrument 2 — the box-side artefact timeline (positive evidence only)

The gate directory's `target/` is a timeline. rustc writes into
`target/<profile>/deps`, `target/<profile>/.fingerprint` and
`target/debug/incremental` only for units it actually rebuilds.

> **SIGNATURE.** For a gate run in directory `D` spanning `[t0, t1]`: count the
> files under `~/D/target/` whose mtime lies in `[t0, t1]`, **excluding
> `target/.rustc_info.json`**.
> `count > 0` ⇒ rustc ran during the run.
> `count == 0` ⇒ *candidate* no-op, and nothing stronger — see the limit below.

`.rustc_info.json` is the exclusion that makes the signature sharp: cargo
rewrites it on **every** invocation whether or not it compiles anything, and it
is the only entry under `target/` that a genuine no-op touches.

### Two-way proof of the signature

Run on the Threadripper in `~/gate-forensics-lab`, on a two-crate workspace,
against the exact cargo invocations `ci/t0.sh` uses. Same broken source bytes in
both arms; **only the mtime differs**:

| arm | source mtime | `fmt` | `clippy` | `check` | `test` | `T0 GREEN`? | `target/` entries changed |
|---|---|---|---|---|---|---|---|
| back-dated (`rsync -a` behaviour) | 2026-08-01 | 0 | **0** | **0** | **0** | yes | **1** — `.rustc_info.json`, nothing else |
| current mtime (the fix's `touch`) | now | 0 | **101** | **101** | **101** | no | **104** — all under `debug/incremental/`, `deps/` |

The negative arm is a tree that does not build being certified green with an
exit code of 0 from every cargo command, and the instrument reads 0. The
positive arm is the identical tree failing honestly, and the instrument reads
104. The signature separates them, and it separates them on the *only* variable
that differs.

Applied to a real gate run known to have compiled — `storage-dir`, `b8a7006`,
2026-08-07 02:36, which merged 4 commits of new Rust — the instrument reads
**1,695**. Applied to the same directory's `.rustc_info.json`-only baseline it
reads 1. The reproduction script is `lab.sh`/`lab2.sh` as recorded in
"Reproduction" below.

### The limit of instrument 2, stated before any verdict is drawn from it

An artefact's mtime records only its **most recent** write. A unit rebuilt by a
later run overwrites the file and erases the earlier timestamp. So
`count == 0` for an old run means "no unit whose last build was during that run",
**not** "no unit was built during that run".

This is not hypothetical: a first pass of this analysis looked only at
`target/debug/deps` and classified 37 of 61 runs as COMPILED NOTHING, including
`collapse-t1-list` (2026-08-05 01:27) — a run that in fact compiled 4,074
artefacts, all of them visible in `target/debug/incremental/` and none of them
in `deps/`, because the `deps/` outputs had been overwritten since. **A
detection instrument whose negative verdict is survivorship-limited is a check
that could not pass, which is the same defect as a check that could not fail
wearing the other hat.** Instrument 2 is therefore used here for *positive*
verdicts only.

### Instrument 1 — reconstruction from git (complete, no survivorship)

The verdicts below rest on this one. It needs no artefact and cannot be eroded
by later builds.

Measured facts about which workflows write an mtime (scratch repos, this
laptop, 2026-08-07 — measured, not recalled):

| workflow | file whose content changes | file whose content does not change |
|---|---|---|
| `git checkout <branch>` | **current mtime** | **old mtime kept** |
| `git merge --ff-only` | current mtime | old mtime kept |
| `git rebase` | current mtime | old mtime kept |
| `git cherry-pick` | current mtime | old mtime kept |
| `git stash` (restore) | current mtime | — |
| `git worktree add` | current mtime — **every file** | current mtime |
| `cp -rn src/. dst/` (handoff stage 1) | current mtime | current mtime |
| `rsync -a` | **source mtime preserved** | not transferred |

So `git checkout` refreshes exactly the files it rewrites, `git worktree add`
refreshes everything, and `cp -rn` — the seeding in handoff stage 1 — **cannot**
back-date anything: GNU `cp` without `-p` stamps the destination with the
current time. (It also only ever touches `data/vanilla`, which nothing
compiles.) The only operation in the whole loop that preserves an old mtime is
the `rsync -a` itself.

Now the reconstruction. `ci/handoff.sh` does `git checkout -q "$BRANCH"` in
`$ROOT` and leaves `$ROOT` there on a red, or on `main` after `git merge
--ff-only "$BRANCH"` on a green — and an ff-merge makes `main`'s tree identical
to the branch's. **Either way `$ROOT`'s tree after run *j* is `tip(j)`.** The
gate directory's tree after run *j* in that directory is also `tip(j)`, because
stage 2 rsyncs `$ROOT` into it with `--delete`.

For each run *i* in directory *d*, with *p* = the previous run in *d*:

* `Transferred(i)` = files differing between `tip(p)` and `tip(i)` — computed
  with `git ls-tree -r` blob comparison, not guessed.
* For each such file, walk the chronological run sequence from *p* to *i* and
  find the last run whose checkout changed that file's blob. That run's start is
  the file's mtime in `$ROOT`.
* The hole fires iff that mtime predates the gate directory's last build.

Two bounds on "the gate directory's last build" were computed, and both give the
same answer:

* **observed** — the latest surviving `target/` write in *d* before run *i*;
* **conservative** — `end(p)`, the previous run's end, which is an upper bound
  on any build it performed and is immune to survivorship.

Result under both bounds: **50 runs "no back-dated file possible", 0 runs with a
back-dated file, 6 runs with no prior run in that directory, 5 runs unresolvable
by diff because a tip has been garbage-collected.** The last two groups are
resolved by hand below.

## CLASSIFICATION OF EVERY GATE RUN ON DISK

Logs live in `~/rustmc-private/gates/` (108 files). Sixty-two carry a
`handoff:` header with a branch, a tip and a gate directory; one of those
(`gate-command-coverage`, 2026-08-07 05:01) was still in flight while this was
written and is excluded. The remaining 46 files are orchestrator-side verdict
captures, stage transcripts from before the header existed, or the proof
driver's log; they name no gate directory and are not classifiable.

**Totals: DID COMPILE 57 · COMPILED NOTHING 0 · CANNOT TELL 4.**

`T0/T1/T2` shows which green sentinels the log contains. `writes` is instrument
2's count (`-` where the directory's `target/` no longer exists).

| start (BST) | gate dir | branch | tip | T0/T1/T2 | writes | verdict |
|---|---|---|---|---|---|---|
| 08-03 17:04 | `rustmc-gate-hard` | `ci-gate-hardening` | `e79acff` | – | – | **CANNOT TELL** (no prior tree known; ran 0 s, no verdict) |
| 08-03 17:06 | `rustmc-gate-hard` | `ci-gate-hardening` | `728414d` | 0 | – | **CANNOT TELL** (prior content of dir unknown) |
| 08-03 17:09 | `rustmc-gate-hard` | `ci-gate-hardening` | `b51d85b` | 0,2 | – | DID COMPILE (I1) |
| 08-03 19:08 | `rustmc-stack` | `oracle-hardening-stack` | `6553f9c` | 0,2 | – | **CANNOT TELL** (prior content of dir unknown) |
| 08-03 22:58 | `rustmc-stack` | `oracle-hardening-stack` | `bcc7ac1` | 0,2 | – | DID COMPILE (I1) |
| 08-03 23:24 | `rustmc-gate` | `oracle-hardening-stack` | `bcc7ac1` | 0,2 | 6 | **CANNOT TELL** — *and it merged* |
| 08-04 02:53 | `rustmc-gate` | `oracle-observed-entity-scope` | `fea2c3a` | 0,2 | 32 | DID COMPILE (I1+I2) |
| 08-04 04:34 | `rustmc-gate` | `upper-half-and-blind-spots` | `4b14811` | 0,2 | 0 | DID COMPILE (I1) |
| 08-04 08:17 | `rustmc-gate` | `move-entity-heartbeat` | `adb7cee` | 0,2 | 20 | DID COMPILE (I1+I2) |
| 08-04 09:55 | `rustmc-gate` | `vacuous-and-motion` | `c45e224` | – | 0 | DID COMPILE (I1); run aborted, no verdict |
| 08-04 09:59 | `rustmc-gate` | `vacuous-and-motion` | `92f1658` | 0,2 | 20 | DID COMPILE (I1+I2) |
| 08-04 15:47 | `rustmc-gate` | `selfcheck-n26-ceiling` | `9911560` | 0,2 | 0 | DID COMPILE (I1) |
| 08-04 16:56 | `rustmc-gate` | `diverge-map-and-power-test` | `e21ce5b` | 0,2 | 0 | DID COMPILE (I1) |
| 08-04 18:46 | `rustmc-gate` | `entities-tracking` | `3b80d0d` | 0,2 | 10 | DID COMPILE (I1+I2) |
| 08-04 20:53 | `rustmc-gate` | `entities-motion` | `faae26f` | 0,2 | 0 | DID COMPILE (I1) |
| 08-04 23:37 | `rustmc-gate` | `menus-and-jump` | `dc95d7f` | 0,2 | 12 | DID COMPILE (I1+I2) |
| 08-05 01:27 | `rustmc-gate` | `collapse-t1-list` | `15901ea` | 0,1,2 | 4074 | DID COMPILE (I1+I2) |
| 08-05 09:42 | `rustmc-gate` | `rule26-movement-and-neighbour-shapes` | `68dfc84` | 0,1,2 | 4 | DID COMPILE (I1) |
| 08-05 11:09 | `rustmc-gate` | `neighbour-shapes-and-coverage` | `901b2e0` | 0,1,2 | 0 | DID COMPILE (I1) |
| 08-05 12:32 | `rustmc-gate` | `chunk-persistence` | `9f3723d` | 0,1,2 | 368 | DID COMPILE (I1+I2) |
| 08-05 14:10 | `rustmc-gate` | `gate-skips-and-scenario-verbs` | `427ece5` | 0,1 | 690 | DID COMPILE (I1+I2) |
| 08-05 15:36 | `rustmc-gate` | `gate-skips-and-scenario-verbs` | `83f0b6e` | 0,1,2 | 0 | DID COMPILE (I1) |
| 08-05 17:27 | `rustmc-gate` | `container-click` | `fe9f2a8` | 0,1,2 | 297 | DID COMPILE (I1+I2) |
| 08-05 19:03 | `rustmc-gate` | `container-click` | `fe9f2a8` | 0,1,2 | 0 | DID COMPILE (I1) — **legitimately compiled nothing: same tip re-gated, 0 files transferred** |
| 08-05 20:08 | `rustmc-gate` | `shape-family` | `d198028` | 0,1,2 | 0 | DID COMPILE (I1) |
| 08-05 22:40 | `rustmc-gate` | `chain-cascade-signs` | `dc9712c` | 0,1 | 0 | DID COMPILE (I1) |
| 08-06 00:03 | `rustmc-gate` | `chain-cascade-signs` | `dc9712c` | 0,1 | 0 | DID COMPILE (I1) — same tip re-gated, 0 files transferred |
| 08-06 01:23 | `rustmc-gatefix` | `ci-gate-hang-and-evidence` | `36b5f48` | – | 6091 | DID COMPILE (I2; cold directory, full build) |
| 08-06 01:26 | `rustmc-gatefix` | `ci-gate-hang-and-evidence` | `726f313` | 0 | 4319 | DID COMPILE (I1+I2) |
| 08-06 01:33 | `rustmc-gatefix` | `ci-gate-hang-and-evidence` | `ecccca6` | 0,1,2 | 4 | DID COMPILE (I1) |
| 08-06 02:15 | `rustmc-gate` | `ci-gate-hang-and-evidence` | `ecccca6` | 0,1,2 | 0 | DID COMPILE (I1) — **cross-directory case, see below** |
| 08-06 03:07 | `rustmc-gate` | `bench-boot-and-rss` | `99cad58` | 0,1,2 | 0 | DID COMPILE (I1) |
| 08-06 03:48 | `rustmc-gate` | `tile-retirement` | `b36fcf7` | 0,1,2 | 2 | DID COMPILE (I1) |
| 08-06 04:29 | `rustmc-gate` | `blockentities` | `4773bd8` | – | 0 | DID COMPILE (I1); red at T0 in 11 s |
| 08-06 04:44 | `rustmc-gate` | `blockentities` | `35fa078` | 0,1,2 | 561 | DID COMPILE (I1+I2) |
| 08-06 05:39 | `rustmc-gate` | `chain-cascade-signs` | `7f24b43` | 0,1,2 | 37 | DID COMPILE (I1+I2) |
| 08-06 06:30 | `rustmc-gate` | `gameplay-corpus-601-611` | `22ed04a` | 0,1,2 | 82 | DID COMPILE (I1+I2) |
| 08-06 10:39 | `rustmc-gate` | `chain-signs-eviction-swing` | `380397f` | 0,1,2 | 135 | DID COMPILE (I1+I2) |
| 08-06 11:32 | `rustmc-gate` | `worldgen-wide-parity` | `d3b74cc` | 0,1,2 | 605 | DID COMPILE (I1+I2) |
| 08-06 12:50 | `rustmc-gate` | `redstone-power` | `caff4e0` | – | 0 | DID COMPILE (I1); red at T0 in 27 s |
| 08-06 12:53 | `rustmc-gate` | `tick-randomtick-selection` | `44594fa` | – | 2 | DID COMPILE (I1); red at T0 in 20 s |
| 08-06 13:03 | `rustmc-gate-tables` | `redstone-power` | `caff4e0` | – | 4417 | DID COMPILE (I2; cold directory, full build) |
| 08-06 13:04 | `rustmc-gate-tables` | `tick-randomtick-selection` | `44594fa` | – | 12 | DID COMPILE (I1+I2) |
| 08-06 13:08 | `rustmc-gate-tables` | `proof-redstone-power` | `270b7c7` | 0,1 | 6588 | DID COMPILE (I2; tip gc'd, argument below) |
| 08-06 13:39 | `rustmc-gate-tables` | `proof-redstone-power` | `270b7c7` | 0,1,2 | 0 | DID COMPILE (I1) — same tip re-gated, 0 files transferred |
| 08-06 14:27 | `rustmc-gate-tables` | `proof-tick-randomtick` | `d61e804` | 0,1,2 | 2236 | DID COMPILE (I2) |
| 08-06 14:36 | `rustmc-gate` | `gate-tables-before-t0` | `4e9abc0` | 0,1,2 | 0 | DID COMPILE (I1) — **cross-directory case, see below** |
| 08-06 15:16 | `rustmc-gate-tables` | `gate-tables-before-t0` | `4e9abc0` | 0,1,2 | 3713 | DID COMPILE (I2) — **concurrency case, see below** |
| 08-06 15:32 | `rustmc-gate` | `redstone-power` | `cc4470a` | 0,1,2 | 44 | DID COMPILE (I1+I2) |
| 08-06 16:28 | `rustmc-gate` | `tick-randomtick-selection` | `ab27878` | 0,1,2 | 227 | DID COMPILE (I1+I2) |
| 08-06 17:25 | `rustmc-gate` | `entity-move-collision` | `9b4afd4` | 0,1,2 | 119 | DID COMPILE (I1+I2) |
| 08-06 18:22 | `rustmc-gate` | `village-savanna` | `7e62cff` | 0,1,2 | 469 | DID COMPILE (I1+I2) |
| 08-06 19:25 | `rustmc-gate` | `determinism` | `f0416c5` | 0,1,2 | 204 | DID COMPILE (I1+I2) |
| 08-06 21:51 | `rustmc-gate` | `movement-tick` | `fbf0ae6` | 0,1,2 | 120 | DID COMPILE (I1+I2) |
| 08-06 22:55 | `rustmc-gate` | `structures-trial` | `4b0cb80` | 0,1,2 | 1931 | DID COMPILE (I1+I2) |
| 08-06 23:43 | `rustmc-gate` | `structures-ruined-portal` | `ca661b1` | 0,1,2 | 2748 | DID COMPILE (I1+I2) |
| 08-07 00:49 | `rustmc-gate` | `gate-worktree-release` | `cd633d7` | 0,1,2 | 2135 | DID COMPILE (I1+I2) |
| 08-07 01:47 | `rustmc-gate` | `plugin-hook` | `f5dd3a0` | 0,1,2 | 2561 | DID COMPILE (I1+I2) |
| 08-07 02:36 | `rustmc-gate` | `storage-dir` | `b8a7006` | 0,1,2 | 1695 | DID COMPILE (I1+I2) |
| 08-07 03:25 | `rustmc-gate` | `ci-count-drift` | `8be9657` | 0,1,2 | 564 | DID COMPILE (I1+I2) |
| 08-07 04:13 | `rustmc-gate` | `gate-infra-verdict` | `e3ca9ba` | 0,1,2 | 4 | DID COMPILE (I1) |

`I1` = instrument 1 (git reconstruction, complete). `I2` = instrument 2
(artefact timeline, positive only). Rows carrying only `I1` with `writes = 0`
are **not** "compiled nothing": they are runs whose surviving artefacts were
overwritten by later builds, and whose transferred files provably could not
have arrived back-dated.

### The three cases that needed hand resolution

**Two cross-directory runs.** These are the only two runs where the gate
directory's tree and `$ROOT`'s tree were different at the start, so the
`git checkout` no longer refreshed exactly the transferred set.

* `2026-08-06 02:15`, `ci-gate-hang-and-evidence` `ecccca6` into `rustmc-gate`.
  `rustmc-gate` held `dc9712c` from the run that ended 01:05:33. `$ROOT` had
  been moved through `36b5f48` (01:23), `726f313` (01:26) and `ecccca6` (01:33)
  by three runs in `rustmc-gatefix`, so the 02:15 checkout was a no-op. All 23
  transferred files (11 of them `.rs`/`Cargo.*`) differ between `dc9712c` and
  `ecccca6`, so every one of them was rewritten at 01:23, 01:26 or 01:33 —
  each later than 01:05:33, the latest moment `rustmc-gate` could have built.
  **Clean.**
* `2026-08-06 14:36`, `gate-tables-before-t0` `4e9abc0` into `rustmc-gate`.
  `rustmc-gate` held `44594fa` from a run that ended 12:53:56. `$ROOT` had been
  moved through `caff4e0`, `44594fa`, `270b7c7` and `d61e804` by five runs in
  `rustmc-gate-tables`. Two of those tips have since been garbage-collected, so
  the per-file walk is unresolvable — but it is not needed: `$ROOT` held
  `44594fa`'s version of every transferred file at 12:53 and `4e9abc0`'s
  version at 14:36, so each was rewritten somewhere in between, and the
  earliest checkout in that interval is 13:03:10 > 12:53:56. **Clean.**

**Three concurrent-run pairs**, the only shape that can defeat the invariant:

| earlier run | overlapping run |
|---|---|
| 14:27–15:16 `rustmc-gate-tables` `proof-tick-randomtick` | 14:36 `rustmc-gate` `gate-tables-before-t0` |
| 14:36–15:24 `rustmc-gate` `gate-tables-before-t0` | 15:16 `rustmc-gate-tables` `gate-tables-before-t0` |
| 15:16–16:04 `rustmc-gate-tables` `gate-tables-before-t0` | 15:32 `rustmc-gate` `redstone-power` |

Only the first can do damage, and only to the next run in `rustmc-gate-tables`
(the 15:16 one): `$ROOT` moved to `4e9abc0` at **14:36:13**, inside the 14:27
run's build window, so the files that run shipped at 15:16 carried that mtime
rather than a fresh one. This is the single genuine candidate in the whole
record, and it is settled by direct artefact evidence rather than by argument:

* debug artefacts in `rustmc-gate-tables` from the 14:27 run end at
  **14:28:49**, earlier than 14:36:13 → debug units were stale → and indeed the
  15:16 run wrote 3,713 debug artefacts starting 15:16:27.
* release artefacts: the 15:16 run wrote **35** release artefacts at
  **15:29:54–15:30:18** — exactly the shape of "rebuild the changed workspace
  crates". Had the release fingerprints post-dated 14:36:13, cargo would have
  found the sources `Fresh` and written **nothing**. It wrote something,
  therefore the 14:27 run's release build preceded 14:36:13. **Clean.**

The second and third pairs move `$ROOT` at a moment when the *next* run in the
affected directory is either absent (`rustmc-gate-tables` was never used again)
or preceded by a fresh checkout at 15:32:27, after the 14:36 run ended at
15:24:32. **Clean.**

For completeness: none of the five `proof-*` runs authorised anything. The
proof driver's own log (`gates/PROOF-DRIVER.log`) records all three of its
handoffs as `--dry-run`: *"GREEN … (dry run, not merged)"*.

## THE MERGES WHOSE AUTHORISATION IS VOID

**The list is empty.** No gate run in the record shipped a back-dated source
file, so no green was issued by a check that compiled nothing.

One merge is *unevidenced* rather than void, and it is named so that it is not
quietly filed under "clean":

| merge | tip | gate run | why it is CANNOT TELL |
|---|---|---|---|
| `oracle-hardening-stack` → main 1,110 → **1,147** | `bcc7ac1` | 2026-08-03 23:24, `rustmc-gate` | First handoff ever to use `~/rustmc-gate`. That directory already held build artefacts from 07-28 and from **08-03 13:29**, put there by something outside this loop, and its tree at that moment is not recorded anywhere. A file differing from that unknown tree and last rewritten in `$ROOT` before 13:29 would have arrived back-dated. Six surviving `target/` writes at 23:24:21 are too few to settle it either way. |

Three further runs are CANNOT TELL and authorised nothing: 08-03 17:04 and
17:06 (`rustmc-gate-hard`, whose `target/` no longer exists; the 17:04 run
produced no verdict at all) and 08-03 19:08 (`rustmc-stack`, same).

**The consequence of that one gap is bounded to zero by later evidence.**
`bcc7ac1` is an ancestor of every subsequent gated tip. `ci-count-drift`
(`8be9657`, 2026-08-07 03:25, 564 artefacts) and `storage-dir` (`b8a7006`,
02:36, 1,695 artefacts) provably compiled trees that contain it, and `main`'s
current tip `e3ca9ba` is `8be9657` plus one gated commit. So even if that gate
had been vacuous, the code it admitted has since been compiled — the authorisation
would be void, the artefact would not be.

## WHAT IS NOT COVERED, AND SAYING SO PLAINLY

* **`main` is 1,479 commits. The `gates/` log format begins 2026-08-03 17:04,
  with `main` at 1,107.** So **1,107 commits — 75% of the history — were merged
  by a loop that left no record of which gate directory it used or which tip it
  gated.** The surviving logs from that era (`~/rustmc-private/gate-*.log`,
  2026-08-02 to 08-03) are raw T0/T2 transcripts with no header. The mechanism
  was live for all of it: `ci/t0-remote.sh` has excluded `target` and used
  `rsync -a` since `a0fcfdc`, 2026-07-27. **That entire period is CANNOT TELL
  and no amount of work on the surviving logs will change it.** What can be said
  is the same bound as above: every one of those commits is contained in trees
  that later, provably-compiling gates built.
* Runs whose gate directory has since been deleted (`rustmc-gate-hard`,
  `rustmc-stack`) have no artefact evidence at all; instrument 1 still covers
  them wherever the prior tree in that directory is known.
* Two tips (`270b7c7`, `d61e804`) have been garbage-collected — the `proof-*`
  branches were never pushed and are gone from the object store. Both runs are
  resolved by argument and by artefact count, not by diff.
* A file whose content changed while its **size and mtime** both stayed the same
  is not transferred by `rsync`'s quick check at all, so it is neither shipped
  nor touched. That is a different hole from this one, it is unaffected by the
  fix on `gate-command-coverage`, and nothing here measures it.

## EXPOSURE BOUND — when can a file actually arrive back-dated?

From the measured table above, the transferred file must be one that
`git checkout` did **not** rewrite in `$ROOT`, and it must have last been
written before the gate directory's most recent build. Working through the
workflows this project actually uses:

* **Fresh worktree checkout (`git worktree add`)** — writes every file with the
  current mtime, so at the instant of creation it cannot back-date anything.
  It becomes a hazard only as it **ages**: a worktree created on 08-05 and
  synced into a directory built on 08-06 ships every file it has ever had at an
  08-05 mtime. This is the shape that bites hardest, and it is the reason the
  1:1 convention below matters.
* **`git checkout` / `git rebase` / `git cherry-pick` / `git merge --ff-only`** —
  all refresh exactly the files whose content they change and no others. On
  their own, none of them can produce a stale-looking file *relative to a
  directory that was synced from the same tree*, because a file whose content
  differs from that directory is by definition a file one of them rewrote.
* **`cp -rn` seeding in handoff stage 1** — **cannot** back-date: GNU `cp`
  without `-p` stamps the destination with the current time. It also only
  touches `data/vanilla`, which no compiler reads. Ruled out by measurement.
* **`rsync -a`** — the only mtime-preserving step, and therefore the carrier
  rather than the cause.

Which gives the bound:

> **The hole can only fire when the laptop tree that feeds a box directory is
> not the same tree that last fed it, or was modified during that directory's
> previous build window.**

Equivalently: it needs **either** two laptop trees sharing one box directory,
**or** two gates overlapping in time. Both are configuration failures, not
driver failures, and both are invisible.

What the project actually does today keeps the bound satisfied:

* Every agent has its own box directory. Twenty-eight `~/rustmc-*` directories
  carry a `target/`, one per branch or experiment, and each is fed by one
  laptop worktree. `~/rustmc` — `ci/t0-remote.sh`'s default `$DEST`, which every
  agent inner loop would share — has **no `target/` at all**, so nothing has
  ever been built there and the one genuinely shared destination is inert.
* The merge gate uses one directory, `~/rustmc-gate`, fed by one tree, `$ROOT`.

And the two ways the record shows it being broken:

1. **Alternate gate directories.** Five have been used
   (`rustmc-gate`, `-hard`, `-tables`, `rustmc-gatefix`, `rustmc-stack`).
   Alternating between them is what created both cross-directory cases above.
   Neither fired, but neither was safe by design.
2. **Concurrent gates.** Three overlapping pairs on 2026-08-06, all inside one
   afternoon, all with `$ROOT` moving while another gate was mid-build. One of
   them put a checkout inside a build window and came within a few minutes of
   firing. `ci/handoff.sh` has no lock: it refuses a dirty `$ROOT`, but two
   handoffs started minutes apart both see a clean tree.

So: **latent, not theoretical.** The mechanism is real and reproducible on
demand (see the lab table), the merge loop's structure kept it starved of the
one input it needs, and that structure is nowhere written down or asserted. The
fix on `gate-command-coverage` removes the dependence on it, which is the right
outcome — the invariant should not have been load-bearing in the first place.

### The residual worth writing down

`ci/handoff.sh` still has no mutual exclusion, and the invariant this document
relies on for the *past* is exactly the invariant a second concurrent gate
breaks. With the `touch` fix in place the consequence is gone. Without a lock,
the *next* instrument that assumes "the gate directory is a copy of `$ROOT`"
will be wrong for the same reason.

## REPRODUCTION

* **Signature lab** (Threadripper, `~/gate-forensics-lab`, isolated from every
  gate directory): two-crate workspace; ship a good tree and build it warm;
  replace a function body with a call to a function that does not exist;
  `touch -d 2026-08-01` that file; `rsync -a --delete --exclude target`; run
  `ci/t0.sh`'s four cargo commands. Then `touch` the same file to now and run
  them again. Snapshot `find target -printf '%T@ %p\n'` before and after each
  arm and `comm` the two.
* **Instrument 1**: parse `~/rustmc-private/gates/*.log` headers for
  `branch`/`tip`/`gate-dir` and the `handoff: <ISO8601>` line; order by start;
  the previous run in the same gate directory gives the directory's tree, the
  previous run overall gives `$ROOT`'s tree; compare trees with
  `git ls-tree -r` blob hashes; walk the run sequence to date each file's last
  rewrite; compare against the previous run's last `target/` write and, as the
  conservative bound, against its end time.
* **Instrument 2**: `find ~/<gatedir>/target -printf '%T@\n' | cut -d. -f1 |
  sort -n | uniq -c`, then bucket by run window. Exclude
  `target/.rustc_info.json`. Use `target/` whole — `deps/` alone is
  survivorship-blind and produced 37 false COMPILED-NOTHING verdicts on the
  first pass.
* **Workflow mtimes**: throwaway `git init` repos, `touch -d` the tracked files
  to a past date, run the operation, `stat -c %y`.

No gate log was modified. Everything above is read-only over
`~/rustmc-private/gates/`, `~/rustmc-private/*.log`, and `target/` metadata on
the box.
