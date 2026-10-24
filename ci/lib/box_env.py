"""Where a check looks for a repository, and what it does when there is none.

# THE ENVIRONMENT MOST GATES ACTUALLY RUN IN

`ci/t0-remote.sh` rsyncs the working tree to the box with `--exclude '.git'`,
and the preflight path uses `git archive`, which strips it too. So the tree
almost every gate runs in **is not a git repository**, and any check that
shells out to `git` there is asking a question the environment cannot answer.

Three checks were written against a repository and therefore could not succeed
where they actually run. They were found by three separate lanes, none of them
looking for this:

  * `ci/check-cited-audits.py` — `git show <sha>:<path>`. With no repo the
    call fails, and the failure was reported as `CITATION UNREACHABLE ... the
    branch was rewritten or pruned`. That is a red naming a parity failure
    that never happened, on every branch, for as long as it has existed.
  * `ci/figure-provenance.py --check` — `git log -S` over full history. With
    no repo it re-derived nothing and reported **56 of 56** figures as
    `NO WITNESS`.
  * `ci/check-figures.py` rule PROVENANCE-NO-COMMIT — resolves a figure's
    provenance sha against the object database. With no repo every sha in the
    ledger is unresolvable, so an eight-digit sha nobody could recognise by
    eye reds the rule for every branch.

The shape is one this project has now found many times in other clothes: the
check worked, the channel into it did not. What is new here is the dimension.
Nobody had asked what else the gate's *environment* breaks, as opposed to what
its logic gets wrong.

# THE TWO REPAIRS, AND WHY BOTH ARE NEEDED

**A skip is not a repair.** A check that quietly stands down when `.git` is
absent is silently absent in exactly the environment where the gate runs, which
is the same defect through a different door: the stage still prints nothing
that looks different from a stage that ran and passed. Every published green
would then rest on a check that had never executed.

So:

1. `ci/t0-remote.sh` ships a **bare object store** to `$DEST/.gitstore`, and
   `git_store()` finds it. The checks then really run in the box copy, which is
   the outcome that actually protects the figures. Measured 2026-08-18 on this
   box: **6.7 s cold, 0.7 s for every later sync**, against a T0 GREEN of
   65.2 min and a RED of 3.4 min (N=164). It is 0.2% of a green gate.

2. Where a tree was assembled some other way — the hand-rolled rsync in
   `docs/AGENT-BRIEF.md`, a `git archive` preflight, a copy somebody made — the
   store is genuinely absent and the check cannot run. Then, and only then, it
   **skips loudly**: one `BOX-SKIP` line naming the check and the reason, on
   stdout where the gate log keeps it. `ci/box-skip-audit.py` collects those
   lines and reds on any skip that is not declared in
   `ci/checks/box-skips.toml`, so a stage cannot start skipping without a
   commit that says so.

# THE RULE, STATED SO IT CAN BE ENFORCED

**A stage may skip only in an environment that could not have run it, and the
gate fails if it skips in one that could.**

`require_store()` enforces the second half directly. It does not ask "is there
a repo?" and shrug; it distinguishes three states:

    a real repo or a shipped store  -> run. Skipping here is a bug.
    nothing that looks like either  -> the environment cannot answer. Skip loud.
    something that looks like one
      but does not work             -> RED. A broken store is not an absent one.

The third case is the one worth the code. A `.git` that exists and errors — a
worktree pointer to a checkout that has been removed, a store shipped half
way — would otherwise take the skip path and read as "no repo here", which is
how an environment defect turns into a permanent silent pass.
"""

from __future__ import annotations

import os
import subprocess
from pathlib import Path

SKIP_MARK = "BOX-SKIP"


class BoxEnvRed(Exception):
    """A store that is present and broken. Never a skip — see the module head."""


def git_store(root: Path) -> Path | None:
    """The object database this tree can ask, or None if it genuinely has none.

    Order: an explicit override, the tree's own `.git`, then the bare store
    `ci/t0-remote.sh` ships. The override exists so a falsifier can point a
    check at a store it built itself, which is the only way to write a positive
    control that forces the real red in a box-shaped copy.
    """
    override = os.environ.get("RUSTMC_GIT_DIR")
    candidates: list[Path] = []
    if override:
        candidates.append(Path(override))
    candidates.append(root / ".git")
    candidates.append(root / ".gitstore")

    for cand in candidates:
        if not cand.exists():
            continue
        # `rev-parse --git-dir` is the cheapest question that distinguishes a
        # usable object database from a path that merely has the right name.
        # A worktree's `.git` is a FILE holding `gitdir: ...`; if the checkout
        # it points at is gone, the path still exists and every later call
        # fails. That must be RED, not a skip.
        proc = subprocess.run(
            ["git", "--git-dir", str(cand), "rev-parse", "--git-dir"],
            capture_output=True, text=True,
        )
        if proc.returncode == 0:
            return cand
        raise BoxEnvRed(
            f"{cand} exists but git cannot use it as an object database: "
            f"{proc.stderr.strip()}. A broken store is not an absent one — "
            f"repair or remove it. Skipping here would turn an environment "
            f"defect into a permanent silent pass."
        )
    return None


def skip(check: str, reason: str) -> None:
    """Announce that a stage did not run, in a form the audit can count.

    # WHY STDOUT IS NOT ENOUGH, MEASURED 2026-08-18

    The first version of this printed one line on stdout and stopped there.
    `ci/corpus-floor-falsify.sh` then emitted its skip correctly and the gate
    log did not contain it — because `ci/t0.sh:576` pipes that stage's output
    through

        grep -E '^(== |corpus-floor-falsify:)'

    and the marker matches neither pattern. Thirteen stages in `ci/t0.sh`
    filter their output that way: four through a `grep -E` with a fixed
    pattern list, nine through `tail -1` or `tail -2`. Every one of them drops
    a line that is not the summary.

    So the announcement was loud and the channel into the log was not, which is
    the defect this project has now found nine times — `ci/t1.sh`'s `T1 GREEN`
    sentinel printed in 0 of 229 logs, `REGRESSED` matching its emitter in 0 of
    45 lines — and this instance was introduced by the repair for the previous
    one.

    A skip therefore also appends to `$RUSTMC_BOX_SKIP_LOG` when that is set.
    `ci/t0.sh` sets it, truncates it at the start of the run and audits it at
    the end, so a skip reaches the audit whatever any stage does to its own
    stdout. Failure to append is deliberately silent: a gate must not go red
    because a temp file could not be written, and the stdout line is still
    there.
    """
    line = f"{SKIP_MARK}  {check}  {reason}"
    print(line, flush=True)
    log = os.environ.get("RUSTMC_BOX_SKIP_LOG")
    if log:
        try:
            with open(log, "a", encoding="utf-8") as fh:
                fh.write(line + "\n")
        except OSError:
            pass


# # A FALSIFIER THAT MAKES A CHECK SKIP ON PURPOSE OWNS THAT SKIP'S LEDGER.
#
# The rule is written here because it is not discoverable from either side.
# `ci/t0.sh` exports `RUSTMC_BOX_SKIP_LOG` and every child appends to it, so a
# falsifier running a check inside a fixture — a tree with no store, built
# precisely to prove the check stands down there — puts its own injected skip
# into the gate's real ledger. `ci/box-skip-audit.py` then reports
#
#     FORBIDDEN SKIP  check-cited-audits — this environment can run it and it
#     stood down anyway (no git object store in /tmp/cited-norepo.ar4ggt0m)
#
# and a green T0 goes red over a temp path. The audit is right and its input is
# wrong: a falsifier's injected behaviour is not evidence about the tree.
#
# Both falsifiers that do this now point the variable at their own file for
# fixture runs, and leave it alone for runs against the real tree — whose skips
# ARE evidence. It cost two rounds in `ci/check-cited-audits-falsify.py`,
# because the discriminator lived in its `run()` helper and one call site had
# its own `subprocess.run`.


def store_args(store: Path | None) -> list[str]:
    """The `git` argv prefix for this store, for callers that build commands."""
    return [] if store is None else ["--git-dir", str(store)]


def looks_like_a_date(c: str) -> bool:
    """True only for a hex-shaped run a calendar date would produce (YYYYMMDD).

    Provenance is prose, and `20260812` is eight hex digits. A date is never a
    commit, whatever it happens to prefix-match in the object database, so both
    rule 18 in `ci/check-figures.py` and `ci/figure-provenance.py --snapshot`
    skip it as a candidate. Only a VALID date is excluded: `27914806` is a real
    commit with no letter in it and month 91, and refusing every all-digit run
    reddened main on every branch on 2026-08-18. `ci/lib/date-rule-check.py`
    pins both directions.
    """
    if not (c.isdigit() and len(c) == 8):
        return False
    year, month, day = int(c[:4]), int(c[4:6]), int(c[6:8])
    return 1900 <= year <= 2099 and 1 <= month <= 12 and 1 <= day <= 31
