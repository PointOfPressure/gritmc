# COLD START — `merge-driver-coverage-audit`

### COLD START — branch `merge-driver-coverage-audit`, 2026-08-18, area "gate throughput"

**This branch closes a standing question with a negative result: there is no
seventh merge driver worth adding.** It changes no behaviour — it adds the
measurement, the reasoning and the reopening condition to `.gitattributes`,
where the next person to ask will look.

**Why it needed doing.** Merge drivers have been the queue's best-paying
throughput lever: the docregions pair on `ci/checks/gameplay-falsifiable.toml`
and `gameplay-baseline.toml` freed **39 pairs** and saved six recordings
(~4.0 h), and `worklist-merge-driver` took branches that would not rebase onto
main from 26 of 48 to 9. So "which file next?" is a reasonable question and had
been answered twice by inspecting one candidate at a time (`ci/t1.sh`,
`docs/BENCH.md`, both rejected). This answers it by exhaustion instead.

**The instrument.** All 46 queued branches, **all 1035 pairs**, three-way merged
against main with `git merge-tree --write-tree --name-only`, every conflicted
path recorded. No worktree, no rebase, no gate, no corpus lock — about eight
minutes.

**THE ONE TRAP, and it cost this measurement its first pass.** `git merge-tree`
does **not** run custom merge drivers. So the six already-driven files still
report as conflicts under it, and `ci/checks/gameplay-falsifiable.toml` tops the
raw ranking at 50 pairs — a file that already has a driver. **Subtract the
driven files before ranking**, or the sweep simply rediscovers the drivers
already in place and reads as though they were still needed.

**The result.**

    79 of 1035 pairs conflict at all                     (7.6%)
    60 of those 79 conflict ONLY in already-driven files
    19 pairs conflict in an UNDRIVEN file — spread over 20 files

    undriven ranking:  5  ci/t2-regiondiff.sh
                       3  ci/checks/gameplay/README.md
                       2  tools/region-differ/src/main.rs
                       2  tools/oracle-runner/src/trace.rs
                       2  ci/vanilla-reach-census.py
                       1  each — sixteen further files

**Why the top candidate is not one.** `ci/t2-regiondiff.sh` is a **22-line stub
on main** that four branches each grow independently — to 45, 45, 289 and 292
lines. Two of the four agree (blob `919d48dc`); the other two disagree with them
and with each other. That is four branches **rewriting one young file**, not
several branches appending to a shared one, and a driver keeping both sides
would emit the concatenation of two different scripts. It is the `## NOW` case
exactly: the conflict is visible and the silently merged result would not be.

`ci/checks/gameplay/README.md` frees 3, across 12 commits with 12 distinct hunk
starts — scattered edits with no region structure to declare. Everything else
frees a single pair, which is one hand rebase, once.

**Corroborates two earlier calls from a direction they were not made from.**
`docs/BENCH.md` and `ci/t1.sh` were rejected by inspecting their commit shape
(deletions, scattered hunks). `docs/BENCH.md` shows up in this pairwise sweep at
**exactly one pair**, which is independent agreement with a judgement made on
different evidence.

**WHAT I KILLED AS A NULL — do not re-run.** The whole search. The remaining
conflict surface is 19 pairs of 1035 over 20 files. Adding a driver to any of
them buys back a rebase or two at the cost of machinery and a marker line every
future edit must respect.

**The reopening condition, so this is closed rather than merely opinionated:**
re-run the sweep and reopen only if a **single undriven file exceeds ~20 pairs**.
Not because a file feels collision-prone.

**The single next item.** None on this branch — it is one commit and it is done.
The throughput lever that replaces it is `docs/GATE-BATCHING.md` §2.3: gates are
not equal (GREEN 65.2 min, RED 3.4 min), which moves batching's optimum to width
8 with `bisect`.
