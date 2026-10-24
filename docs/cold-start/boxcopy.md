# Cold start — branch `boxcopy`

---

### COLD START — branch `boxcopy` off `f82b2ebb`, 2026-08-18

> **The shas in this file are this branch's own and move when it rebases.**
> `ci/lanes.sh` rebases routinely, so every one of them was orphaned the first
> time it happened and had to be remapped by commit subject. They are kept
> because a classification table whose rows do not say what repaired them is
> half a table; they are not citations in the `ci/checks/cited-audits.toml`
> sense and nothing gates them.

**The area: everything the gate's actual environment breaks that the repo tree
hides.** Gates do not run in a git worktree. `ci/t0-remote.sh` ships an rsync
copy with `--exclude '.git'`, and the `git archive` preflight strips it too. So
the tree almost every gate runs in is not a repository, has no `oracle-work`,
no `data/vanilla` and no `target` — and nobody had asked the general question
of what that costs. Remote dir `~/rustmc-boxcopy`.

**The headline: `origin/main` itself was RED in that copy, and had been all
day.** `ci/check-figures.py` rule 18 sits at `ci/t0.sh:66` and T0 exits on
first failure, so **no stage below line 66 ran remotely at all**. The merge
train stopped at 09:12:50; every branch after it reported `t0=1/0` with T1 and
T2 at 99, never reached. Every remote "T0 GREEN" taken on 2026-08-18 before
this branch landed is a claim about a run that stopped near the top, and every
"T0 RED" is one false red wearing a parity failure's message.

## THE CLASSIFICATION

Method: build the box-shaped copy deliberately, with this repo's own rsync
exclusions, and run every T0 stage, falsifier, selftest and generator in it —
against a git worktree as the control, and against a set-up gate directory as
a third arm. 70 entries swept across three environments.

| stage | class | worktree | box copy | repaired by |
|---|---|---|---|---|
| `ci/check-figures.py` rule 18 | NEEDS-REPO | GREEN | **RED** | `4c18ce24` store, `f0de5d21` date rule |
| `ci/check-cited-audits.py` | NEEDS-REPO | 0 of 4 failing | **4 of 4 failing** | `c5af17c4` |
| `ci/figure-provenance.py --check` | NEEDS-REPO | 0 red | **56 of 56 NO WITNESS** | `2bd25dc2` |
| `ci/check-cited-audits-falsify.py` | NEEDS-REPO | 5/5 | **1 passed, 3 failed** | `c3ab7246` |
| `ci/corpus-floor-falsify.sh` | NEEDS-STATE | 39/0 | **39/0 or 31/9** | `300aa589` (announced) |
| `ci/tick-section-classify.py --selftest` | NEEDS-STATE | traceback | traceback | `2ee97e83` |
| `ci/check-use-probe-falsify.sh` | NEEDS-STATE | 10 passed, 2 cases skipped | 10 passed, 1 case skipped | pre-existing, correct |
| `Cargo.lock` / every cargo resolve | writes tracked state | **dirties the tree** | immune | `5c62c2ed`, `c5c54aa7` |
| `ci/t0.sh` × 13 filtered stages | channel, not check | drops `BOX-SKIP` | drops `BOX-SKIP` | `f9a5653e` |
| `ci/check-figures-falsify.py` | hermetic fixture | GREEN | GREEN | builds its own repo — the model |
| `ci/setup-merge-drivers.sh` | NEEDS-REPO | registers | **loud skip, verifies by text** | already correct — the model |
| `ci/gen-vanilla-region.sh` | shared-state tripwire | — | — | `adb8c59b` |
| `ci/t2-regiondiff.sh` | **not a defect** — see below | — | — | — |
| everything else (58 entries) | — | same | same | no defect |

**NEEDS-NET is not a gate hazard and was checked rather than assumed.** Four
scripts reach the network — `ci/vanilla-server.sh` and `ci/bench-servers.sh`
(`curl`, both guarded on the jar already being present), `ci/handoff.sh` (`git
push`, which is the merge driver's job) and a `git clone` line inside an error
message. None is a T0 stage.

**NEEDS-PATH is real but is instruments, not gates.** ~25 scripts default to
`$HOME/rustmc/oracle-work`, all with env overrides, and that shared tree exists
on the box. `ci/bench-arms-setup.sh` hardcodes five `/home/sayer/grit/work-*`
paths with no override.

## THE STRUCTURAL CHOICE, MEASURED BEFORE IT WAS MADE

Ship a repository, or give the repo-dependent checks a repo-free data path.

**A data path does not survive contact.** All three checks ask questions ABOUT
history — is this sha an object, what did this blob say at that sha, which
commit first published this claim. Anything a `.git`-less tree could carry
instead is the check's own previous answer written down, and a check that
verifies its own cache is the unfalsifiable class this project has twenty-one
instances of.

    rsync -az .git over ssh, cold            612 MB    6.7 s
    rsync -az .git over ssh, incremental               0.7 s
    the same content as a reachable pack      47 MB    3.95 s to build
    first --sync, which creates the store               9.22 s
    --sync into a FRESH gate directory after that       2.52 s
    T0 GREEN, for scale                                65.2 min  (N=164)
    T0 RED, for scale                                  3.4 min

**2.52 s is the number that matters**, because the store is created once per
box and every gate directory after the first pays only for its 124-byte
pointer. Verified on a directory that did not exist: cold sync, then
`check-cited-audits` 4 citations 0 failing, `FIGURES GREEN`, and
`figure-provenance` re-deriving 56 figures — all three running for real in a
tree with no `.git`.

612 MB is **93% unreachable loose objects** — git says so on every fetch. A
`git gc` on `~/grit/repo` would take the cold figure to about 47 MB; that is
the owner's call on a live repository with ten lanes writing to it.

**What ships is not a copy of `.git` per gate directory** — that would be 18 GB
across thirty of them, and a worktree's `.git` is a pointer file naming a
checkout the box does not have. One store on the box, and a bare repository of
**124 bytes** in each gate directory whose `objects/info/alternates` names it.
Measured: `cat-file`, `show <sha>:<path>` across branches, and `log -S` over
full history all resolve through it.

Sharing an object store is safe in a way sharing `oracle-work` or
`data/vanilla` is not. Those are mutable and mean different things on different
branches, so a shared one answers with whoever wrote last. An object database
is content-addressed and append-only: no branch can make another read a wrong
answer out of it, and the only failure it admits is an absent object, which is
loud.

## THE SKIP RULE

    A stage may skip only in an environment that could not have run it,
    and the gate fails if it skips in one that could.

A silent skip would make a stage absent exactly where every gate runs, which is
the same defect by another door. So a skip is one `BOX-SKIP` line,
`ci/checks/box-skips.toml` declares the six that may appear with a reason
each, `ci/box-skip-audit.py` reds on any it does not, and `ci/t0.sh` passes
`--require-ran` built by asking, for each declared skip, the same question the
stage asked — a store present, a floor file present, the jar present, a clean
`Cargo.lock` in a repo. Without that second half "declared" drifts into
"permanently excused".

A row also comes back **out** when the stage learns to decide instead.
`check-figures:provenance` was declared, then removed once
`_looks_like_a_date` could tell `27914806` from `20260810`: a declared skip
that can never fire reads as coverage somebody checked and nothing would ever
exercise it. Six rows, each reachable.

**Two lanes reached four of these repairs independently.**
`origin/gatefix-gitless-checks` fixed `check-cited-audits`,
`figure-provenance`, that check's falsifier and rule 18 in parallel with this
branch — eleven conflicting regions across four files. Their rule 18 is better
and this branch adopts it. Recorded because two lanes spending a morning on one
cause is a fact about how the work was split, not a merge to resolve quietly.

## THE ORDERING DECISION

**T0 exiting on first failure is a magnifier and the ordering should change,
but not everywhere.** Measured: the pre-cargo block costs ~166 s; a GREEN is
65.2 min and a RED 3.4 min (N=164). Running the whole cheap block and
collecting failures raises the worst-case cost of a cheap red from seconds to
166 s, against a 65-minute green, and stops one environment fault at line 66
hiding thirty other stages — which is exactly what it did for a day. The cargo
block should keep first-failure: those stages are minutes each and a clippy red
makes the test stages meaningless.

**Implemented, and it was not a one-line reorder.** Routing all 37 failure
paths through a counter did nothing on its own: `ci/t0.sh` runs under `set -e`
AND `set -o pipefail`, and `set -e` is suppressed only for the command left of
`||`, not for the `{ ... }` on its right. Almost every group there re-runs the
failing command to show its output, that re-run fails by construction, and the
script died before the counter was reached. 35 diagnostic re-runs now carry
`|| true`. One of them was already ending T0 with **exit 2 instead of 1** before
this branch, invisible because both are non-zero.

The structural cases were green throughout that; only an end-to-end run with an
injected failure, looking for a stage far below it, caught it. And the first
injection was itself a no-op — a bogus flag on `ci/check-figures.py`, which
ignores unknown argv — so the case was measuring nothing until it used `false`.

`ci/t0-ordering-falsify.sh` case 3 is the ratchet: a new stage added with a bare
`exit 1` would look exactly like the stages around it and silently restore the
masking.

## WHAT IS LOGGED FOR OTHER AREAS

**`ci/corpus-floor.py` has four cases that cannot fire on a real floor file**,
found because the two routes were being compared. On pristine `origin/main`
with a saved measurement whose ensemble AND projection both match the committed
floor file, `ci/corpus-floor-falsify.sh` gives 31 passed / 9 failed where the
synthetic route gives 39 / 0. The measurement carries no `vocabulary` key at
all, so cases 22, 23, 24 and 27 — which remove or drift a vocabulary and expect
`NO VOCABULARY` or `VOCABULARY STALE` — inject nothing into a file that already
has nothing and the tool exits 0 saying nothing. Cases 1, 21 and 30 fail
against it too. **Belongs to the corpus floor area**; this branch only stopped
the two routes reporting as one.


## `ci/t2-regiondiff.sh`, HANDED OVER AS A ZERO-FLAKE GATE THAT FLAKES

It is not, on the evidence available here, and the two things found on the way
are worth more than the original question.

**What looks like 146 measurements is one.** Every gate directory's
`oracle-work/t2-regiondiff` is a symlink to a single shared directory harvested
2026-07-22 at 19:55. All 146 resolve to it and are byte-identical from every
direction. Diffing all of them yields 146 exact pairs and that number means
nothing. It was very nearly written down as a sample.

**The differ is exact.** A harvest against a byte copy of itself is
100.000000%, so any flake is in generation, not in comparison. The reported
divergence (94 blocks, seagrass and `cinnabar`↔`sulfur`) carries the signature
of a **tracked ours-vs-vanilla divergence** — `sulfur -> cinnabar` already has
three dedicated instruments in this tree — so which two things were being
diffed is worth settling before this gate's declaration is weakened. Its window
is R=2 with an inner margin of 1, centred away from world spawn *precisely
because* vanilla's harvest is not deterministic at larger radii; the comment
saying so predates the flake report.

## A CLAIM THIS BRANCH GOT WRONG

The symlink guard in `ci/gen-vanilla-region.sh` was first written up as
preventing a catastrophe: one run deleting the shared references for every
agent at once. That is the intuitive answer and it is false.

    rm -rf link      -> removes the LINK. The target survives.
    rm -rf link/     -> removes the TARGET'S CONTENTS.
    rm -rf link/*    -> the same.

There is no trailing slash on that line, so it was never the catastrophe.
`ci/shared-refs-falsify.sh` case 0 refuted it before it was committed.

**The asymmetry is the real finding**: one character between "removes a link"
and "empties 2.3 GB that 146 trees share", in the same links and for the same
reason as the rsync exclude pattern in `ci/t0-remote.sh`'s header — which HAS
fired, and cost 200 MB over the wire plus a re-extract. The guard is a tripwire
on that class, not a fix for a bug in that line.
