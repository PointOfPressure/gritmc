# The backlog is four numbers, and only one of them is the backlog

Measured 2026-08-19 against `origin/main` at **`67f5fc71e`** (main 2250),
**286 branches**, with `ci/unlanded.sh`.

| number | what it counts | the question it answers |
|---:|---|---|
| **4,182** | sum of `git rev-list --count origin/main..<b>` over every branch | *"how many commit objects sit on branches ahead of main, counted once per branch carrying each one?"* A commit on five stacked branches is counted five times. This answers no question about a quantity of work. |
| **2,587** | `git rev-list --count <all branches> --not origin/main` | *"how many distinct commit OBJECTS are unreachable from main?"* De-duplicated by sha, so the stacking is gone — but a commit whose patch landed under a rebase has a new sha and is still counted, and this repository merges by rebase. |
| **2,488** | `git cherry` `+` lines, distinct shas | *"which commits are not on main by patch-id?"* Drops the rebased-and-landed ones. Two branches carrying the same cherry-picked fix still contribute two shas. |
| **1,377** | distinct patch-ids | ***"how many distinct changes are not on main?"*** **This is the backlog.** |

**Quote 1,377, and quote the sha with it.** A backlog figure without the commit
it was measured against is not re-checkable, because `origin/main` moves while
you measure.

## Why the wrong one is so easy to reach for

`4,182` is what `git rev-list --count` gives you and it is **3.0x** the real
figure. The error is not a constant: it scales with how stacked the branch set
is, so it is largest exactly when a backlog reading matters most.

This run found the extreme case. The twelve `origin/wip/gpland/*` branches are
**one stack, not twelve** — `gameplay-backlog-landing` contains all eleven
others as ancestors. Counting them separately adds **786** branch-commits to a
single 136-patch body of work. One naming convention, most of the gap.

## 1,377 is a ceiling, not an identity

`git cherry` compares patch-ids computed over diff hunks, and a hunk carries
its context lines. A commit replayed over an unrelated edit to the **same
region** gets a different patch-id and reads as unlanded.

Demonstrated on this lane's own stack rather than argued: `land-gameplay`
carries **all 136** of `gameplay-backlog-landing`'s commit subjects, zero
missing, and `git cherry` still reports **10** of them outstanding. Where the
two disagree, compare by subject.

## The number went UP while work landed

The same measurement on 2026-08-19 against main **2232**, 283 branches, was
**1,342**. Against main **2250**, 286 branches, it is **1,377**.

Eighteen commits landed and the backlog grew by 35, because three new branches
appeared in the same window. **The queue is not a fixed pile being drained.**
Any plan that treats it as one — including any ordering strategy — is solving
the wrong shape of problem, which is independently what the queue-drain lane
concluded when it found that ordering cannot shrink a serial drain.

## What this shares with the defects this project keeps finding

A right number applied to a question it does not answer:

* `108/108` counted **declarations** and was read as **coverage**;
* `union_N` measured the **width of a mask** and was read as a **null**;
* `rev-list --count` counts **commit objects per branch** and is read as
  **unlanded work**.

The third is the one that reaches planning, because it is the one a person
types when they want to know how much is left.
