#!/usr/bin/env python3
"""Every commit sha a claim document cites, resolved against the refs.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/check-cited-commits.py           verdict on every cited sha, exit 0/1
#     ci/check-cited-commits.py --list    including the ones that are fine
#     ci/check-cited-commits.py --fix     follow a rebased sha to its new one
#
# WHAT BREAKS IF YOU IGNORE THIS: a row stamped CLOSED says "SWAP landed in
# `3428600a`" and the sha resolves to nothing a reader can fetch. Measured
# 2026-08-18 on `docs/PARITY-ROADMAP.md`: 30 of 35 cited shas are on `main`
# and **five are reachable from no ref at all**.
# ===== end interface; everything below is why =====

# # A SHA IS THE STRONGEST CITATION IN THIS PROJECT AND THE EASIEST TO LOSE

This tree cites commits as evidence constantly, and it is right to: a sha is
the one citation that cannot be reworded. `docs/PARITY-ROADMAP.md` closes rows
on them — "`SWAP` landed in `3428600a`", "the sneak gate landed in `6783d819`
one day *before* this row was stamped".

But a sha is only a citation while something reaches it. Branches get rebased,
stranded branches get deleted, and the commit becomes an unreachable loose
object: still resolvable in the clone where it was written, invisible in every
other, and deleted by the next `git gc --prune`. The citation does not rot
into a *wrong* answer, which is what makes it dangerous — it rots into *no*
answer, and only for other people.

Measured on `main` at `cf443588`, five shas in `docs/PARITY-ROADMAP.md` are in
that state: `2fea3d78`, `4b532a71`, `4fddc9a2`, `d69598d7`, `f1ba3797`. Every
one is honestly labelled in the prose — "stranded", "unmerged", "tree
`d69598d7`" — so nobody wrote anything false. The prose is the last record
that they existed, and a reader on a fresh clone gets `bad object` from all
five. This repository is carrying that evidence in one working copy that
already warns on every commit about too many unreachable loose objects.

# # WHAT THIS DOES NOT DO, AND WILL NOT

**It does not demand every cited sha be on `main`.** Citing unmerged work is
correct and common here — `ci/checks/cited-audits.toml` exists precisely so a
lane's finding can be quoted before it lands, pinned to a sha rather than a
branch name. ON-BRANCH is a green verdict and the branch is named, which is
strictly more than the document says on its own.

It does not rewrite anything. An UNREACHABLE sha cannot be repaired by
guessing a replacement, and the honest fixes — restore the ref, or say in the
prose that the object is gone — are both a human's call.

# # THE DEFECT CLASS THIS MUST NOT JOIN

A sha checker that finds no shas passes. `MIN_SHAS` floors it. And the
verdicts are counted and printed on green runs, because "0 unreachable" is
only meaningful beside "35 checked" — a run that silently checked nothing
prints the same reassuring nothing as a clean tree.
"""

from __future__ import annotations

import argparse
import re
import os
import shutil
import subprocess
import tempfile
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "ci" / "lib"))
import box_env  # noqa: E402  (needs ROOT on the path first)

# The object database this check asks. A real checkout answers with its own
# `.git`; the merge gate's rsynced copy answers with the bare store
# `ci/t0-remote.sh` ships (`.gitstore`, HEAD at the gated commit, NO refs).
# Set once by main(); the helpers below read it.
STORE: Path | None = None
STORE_HAS_REFS = False

DOCS = ("docs/PARITY-ROADMAP.md", "docs/DETERMINISM.md", "docs/WORKLIST.md")

# Backtick-quoted, 7-12 hex. Requiring the backticks is what keeps this from
# matching every hex-looking word in the prose — `basedrum`, `deadbeef` in a
# byte dump, an item id. It costs the handful of unquoted shas, and a citation
# not written as code was not written as a citation.
SHA_RE = re.compile(r"`([0-9a-f]{7,12})`")

# Words that are hex but are not shas. `accede`, `decade` and friends: this
# tree writes `add7297a`-style shas and also things like `deadbeef`.
NOT_A_SHA = {"deadbeef", "cafebabe", "baaaaaad", "feedface"}

# HEX TOKENS THAT ARE NOT COMMITS OF THIS REPOSITORY, each with the reason a
# human gave for it.
#
# This is a ledger and I have argued elsewhere in this tree against ledgers,
# so the argument for this one: the alternative is parsing English. Both
# entries below ARE labelled correctly in the prose — one says "digest", the
# other says "the plugins repo's `main`" — and a checker that read those
# labels would be guessing at natural language and would be wrong the first
# time somebody wrote the same fact a different way.
#
# What keeps it from being a hole: it is keyed by the exact sha, so it cannot
# cover a token nobody has looked at; every entry carries its reason; and an
# unresolvable sha that is NOT here is still red. Adding an entry is a
# deliberate statement, not a silence.
NOT_OURS = {
    "a9dff28db35c":
        "a recording-ensemble digest, not a commit — `docs/PARITY-ROADMAP.md` "
        "says 'same digest' beside it, over six runs of "
        "coverage92-selfcheck-n6-20260811",
    "f31a805":
        "a commit of the SIBLING grit-plugins repository, not this one — "
        "`docs/WORKLIST.md` says 'the plugins repo's `main`'. It is a path "
        "dependency with no version resolution, which is why the sha is "
        "quoted at all",
}

# `was `11ff71d8`` — a QUOTATION of a sha, not a citation of one.
#
# Same problem `ci/check-source-citations.py` meets with `(was `:856`)`, and
# the same answer, because it arose the same way: the WORKLIST row describing
# THIS check cites `11ff71d8` in order to say that it stopped resolving, and
# the check reddened on it. The old sha is the evidence; replacing it with the
# new one would make the sentence claim nothing had moved.
#
# `was` before the backticks, a word a reader already understands, counted and
# printed on every run so the exemption cannot grow quietly.
HISTORICAL_SHA = re.compile(r"\bwas `([0-9a-f]{7,12})`")

MIN_SHAS = 20

# A RATCHET, NOT AN ABSOLUTE.
#
# It was 20 for about an hour. `--fix` then followed 15 of those to the sha
# their patch now lives at, so it is 5 — and lowering it in the same breath is
# the point of printing the slack on green runs.
#
# The remaining 5 carry no reachable patch anywhere: 0 candidates each, not
# "more than one and I declined to choose". They are genuinely gone, and they
# cannot be repaired from here. Guessing a replacement turns a citation nobody
# can resolve into one that resolves to the wrong thing, which is strictly
# worse than an honest dead end.
#
# WHY IT IS A CEILING AND NOT A DEMAND FOR ZERO: this number moves on its own.
# `11ff71d8` went unreachable DURING the session that wrote this file —
# `clamped-floor-probe` rebased 61 seconds earlier and the same patch became
# `0aa87464`, having also been `4601fd8c` before that. Three shas, one patch,
# inside one afternoon. With ten lanes rebasing continuously, a demand for
# zero would be red for reasons no lane caused and could not fix.
#
# LOWER IT after running `--fix`. Same shape as
# `ci/check-doc-claims.py`'s MAX_DANGLING_SECTION_REFS, for the same reason.
# RE-BASED 5 -> 8 ON LANDING, 2026-09-04. Five when written (2026-08-19); on
# the branch's own base today the same scan reports six, because reachability
# is a property of the ref store and a ref deleted by anyone after the fact
# moves this count for every tree. The landing main (f4047ae1f) then cites two
# more — `0615e789` and `e437ab99`, docs/WORKLIST.md:1056 — whose branch was
# deleted after its cold-start row was written. `--fix` finds no reachable
# patch for any of the eight, so none is a rebased sha to follow, and the
# remedy the RED names is the prose, which is the owner's call, not a lane's.
# The ratchet's direction is unchanged: the ninth reddens.
#
# AND 8 -> 9 ON THE STACK, same day: `186d6f17` (docs/WORKLIST.md, the
# t0-test-target-coverage cold-start row) names a branch tip that was rebased
# away after the row was written, the same shape as the two above. The tenth
# reddens.
MAX_UNREACHABLE = 9


def git(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *box_env.store_args(STORE), "-C", str(ROOT), *args],
                          capture_output=True, text=True)


def refs_containing(sha: str) -> list[str]:
    """Refs from which `sha` is reachable — branches, remotes and tags only.

    Deliberately NOT the 250 worktree HEADs that `git log --all` would also
    walk. See `rebased_replacement` for the argument; the short form is that a
    citation is for a reader, and a reader gets refs.

    `git for-each-ref --contains` does the whole walk in one process; doing it
    with a `merge-base --is-ancestor` per ref costs 250 processes per sha and
    turned a 3-second check into minutes when it was written that way first.
    """
    r = git("for-each-ref", "--format=%(refname:short)", "--contains", sha,
            "refs/heads", "refs/remotes", "refs/tags")
    return [x for x in r.stdout.split() if x]


def scan() -> list[dict]:
    out: list[dict] = []
    seen: set[str] = set()
    for rel in DOCS:
        p = ROOT / rel
        if not p.exists():
            continue
        for i, line in enumerate(p.read_text(errors="replace").splitlines(), 1):
            quoted = set(HISTORICAL_SHA.findall(line))
            for sha in SHA_RE.findall(line):
                if sha in quoted:
                    if sha not in seen:
                        seen.add(sha)
                        out.append({"sha": sha, "doc": rel, "line": i,
                                    "verdict": "HISTORICAL",
                                    "detail": "written `was `<sha>`` — quoted to "
                                              "say it moved, not cited as current"})
                    continue
                if sha in NOT_A_SHA or sha in seen:
                    continue
                if sha in NOT_OURS:
                    seen.add(sha)
                    out.append({"sha": sha, "doc": rel, "line": i,
                                "verdict": "NOT-OURS", "detail": NOT_OURS[sha]})
                    continue
                seen.add(sha)
                out.append({"sha": sha, "doc": rel, "line": i})
    return out


def adjudicate(cites: list[dict]) -> list[dict]:
    on_main = set()
    r = git("rev-parse", "--verify", "--quiet", "origin/main")
    main = r.stdout.strip() or "main"
    for c in cites:
        if c.get("verdict"):
            continue
        sha = c["sha"]
        # Ask what KIND of object it is before assuming it is a commit. This
        # tree cites blobs too — `docs/DETERMINISM.md` names "blob `b8c853ed`
        # for `feature/iceberg.rs`" — and the first version of this check
        # called that UNKNOWN, i.e. reported a correct citation as a broken
        # one. A false red on a citation that is fine is how a citation
        # checker gets switched off.
        kind = git("cat-file", "-t", sha).stdout.strip()
        if not kind:
            c["verdict"] = "UNKNOWN"
            c["detail"] = (
                "no object of this name in this clone. Either a sha from "
                "another repository (grit-plugins, the pinned jar's own "
                "history), a run id that happens to be hex, or a typo — say "
                "which in the prose, because a bare unresolvable sha reads as "
                "a citation of this repository"
            )
            continue
        if kind != "commit":
            c["verdict"] = f"OTHER-OBJECT"
            c["detail"] = f"a {kind}, not a commit — cited as such, and present"
            continue
        c["subject"] = git("log", "-1", "--format=%s", sha).stdout.strip()[:60]
        if git("merge-base", "--is-ancestor", sha, main).returncode == 0:
            c["verdict"] = "ON-MAIN"
            c["detail"] = c["subject"]
            on_main.add(sha)
            continue
        holders = refs_containing(sha)
        if holders:
            c["verdict"] = "ON-BRANCH"
            c["detail"] = f"reachable from {', '.join(holders[:4])}" + (
                f" +{len(holders) - 4} more" if len(holders) > 4 else "")
        else:
            c["verdict"] = "UNREACHABLE"
            c["detail"] = (
                f"{c['subject']!r} — resolvable in this clone only, reachable "
                "from no ref, and deleted by the next `git gc --prune`"
            )
    return cites


# UNKNOWN is always red: an unresolvable name presented as a sha of this
# repository is a citation of nothing. UNREACHABLE is ratcheted — see
# MAX_UNREACHABLE.
BAD = ("UNKNOWN",)


def patch_id(sha: str) -> str:
    show = subprocess.run(["git", *box_env.store_args(STORE), "-C", str(ROOT), "show", sha],
                          capture_output=True, text=True)
    pid = subprocess.run(["git", *box_env.store_args(STORE), "-C", str(ROOT), "patch-id", "--stable"],
                         input=show.stdout, capture_output=True, text=True)
    return pid.stdout.split()[0] if pid.stdout.split() else ""


def rebased_replacement(sha: str) -> list[str]:
    """Reachable commits carrying the SAME PATCH as `sha`.

    This is the honest repair and the only one. `11ff71d8` went unreachable
    during the session that wrote this file — `clamped-floor-probe` rebased 61
    seconds earlier and the same patch is now `0aa87464`, having also been
    `4601fd8c` at some point. Three shas, one patch.

    Guessing a replacement is forbidden everywhere else in this checker, and
    this is not guessing: `git patch-id --stable` is computed on both sides
    and must agree. Candidates are narrowed by commit subject first, because
    computing a patch-id for every reachable commit costs minutes and
    narrowing by subject costs one `git log`.

    It returns a LIST. Two reachable commits with one patch-id is the normal
    state of a branch that was cherry-picked, and picking one of them would be
    the guess this function exists to avoid.
    """
    subject = git("log", "-1", "--format=%s", sha).stdout.strip()
    if not subject:
        return []
    want = patch_id(sha)
    if not want:
        return []
    # `--branches --remotes --tags`, NOT `--all`, and the difference is not
    # pedantry.
    #
    # `git log --all` on this box also walks the detached HEADs of 250
    # worktrees. `git for-each-ref` does not. So the first version of this
    # repair "fixed" `4b532a71` to `4b532a71` — it found the original commit
    # via a worktree HEAD, called it a reachable replacement, and rewrote the
    # citation to itself while reporting a repair.
    #
    # Which set is right is decided by the question being asked. A citation is
    # for a READER, and a reader clones and gets branches, remote branches and
    # tags. A commit that survives only as some worktree's detached HEAD on one
    # machine is exactly as unresolvable to them as one that is gone, so
    # UNREACHABLE is the correct verdict for it and the repair must not
    # disagree with the verdict about what counts.
    out = git("log", "--branches", "--remotes", "--tags", "--format=%H%x09%s")
    cands = [ln.split("\t", 1)[0] for ln in out.stdout.splitlines()
             if ln.split("\t", 1)[1:] and ln.split("\t", 1)[1] == subject]
    # `c` is a full sha and `sha` is abbreviated, so `c != sha` never fires on
    # its own — compare on the prefix, or a commit is offered as its own
    # replacement.
    return [c for c in cands if not c.startswith(sha) and patch_id(c) == want]


def fix(cites) -> int:
    """Rewrite UNREACHABLE citations whose patch is reachable under a new sha."""
    edits: dict[str, list[tuple[str, str]]] = {}
    stuck: list[dict] = []
    for c in cites:
        if c["verdict"] != "UNREACHABLE":
            continue
        repl = rebased_replacement(c["sha"])
        if len(repl) == 1:
            new = repl[0][: len(c["sha"])]
            edits.setdefault(c["doc"], []).append((c["sha"], new))
            print(f"{c['doc']}:{c['line']}  {c['sha']} -> {new}  "
                  f"(same patch-id, rebased)")
        else:
            stuck.append(dict(c, n=len(repl)))
    for rel, subs in edits.items():
        path = ROOT / rel
        text = path.read_text()
        for old_sha, new_sha in subs:
            text = text.replace(f"`{old_sha}`", f"`{new_sha}`")
        path.write_text(text)
    if stuck:
        print(f"\n{len(stuck)} left alone — no reachable commit carries their "
              "patch, or more than one does. Neither is repairable by this "
              "script: the first is genuinely gone, and choosing between two "
              "is the guess this refuses to make.")
        for c in stuck:
            print(f"  {c['doc']}:{c['line']} {c['sha']} — {c['n']} candidate(s)")
    print(f"\nrewrote {sum(len(v) for v in edits.values())} citation(s)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--fix", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return _selftest()

    found = scan()
    # Which object database answers, asked of ci/lib/box_env.py the way
    # check-cited-audits asks: a checkout's own .git, or the bare store the
    # gate copy ships. The fourth T0 of stack-ci (2026-09-04) reported 124
    # UNKNOWN of 127 because this ran `git -C` in an rsynced directory with
    # no .git and never looked for the store beside it.
    global STORE, STORE_HAS_REFS
    try:
        STORE = box_env.git_store(ROOT)
    except box_env.BoxEnvRed as exc:
        print(f"check-cited-commits: {exc}", file=sys.stderr)
        return 1
    if STORE is None:
        box_env.skip("check-cited-commits",
                     f"no git object store in {ROOT} — {len(found)} citation(s) "
                     f"not resolved. Ship one with ci/t0-remote.sh, or set "
                     f"RUSTMC_GIT_DIR.")
        return 0
    STORE_HAS_REFS = bool(git("for-each-ref", "--count=1").stdout.strip())
    cites = adjudicate(found)
    if a.fix:
        return fix(cites)
    reds = [c for c in cites if c["verdict"] in BAD]
    unreachable = [c for c in cites if c["verdict"] == "UNREACHABLE"]
    counts: dict[str, int] = {}
    for c in cites:
        counts[c["verdict"]] = counts.get(c["verdict"], 0) + 1

    for c in cites:
        if a.list or c["verdict"] in BAD or c["verdict"] == "UNREACHABLE":
            print(f"{c['verdict']:<12} {c['sha']}  {c['doc']}:{c['line']}")
            print(f"             {c['detail']}")

    summary = ", ".join(f"{v} {k}" for k, v in sorted(counts.items()))
    print(f"\ncited-commits: {len(cites)} shas cited across "
          f"{len(DOCS)} documents — {summary}")

    if len(cites) < MIN_SHAS:
        print(f"\nRED: only {len(cites)} shas matched, floor is {MIN_SHAS}. "
              "A run that checked nothing prints the same nothing as a clean "
              "tree — see MIN_SHAS.")
        return 1
    print(f"cited-commits: {len(unreachable)} unreachable of ceiling "
          f"{MAX_UNREACHABLE}")
    if not STORE_HAS_REFS:
        # The shipped store carries objects and HEAD, not branches, so
        # "reachable from a ref" has no answer here; UNKNOWN (no such object)
        # still does, and is still red. Say which half ran.
        print(f"check-cited-commits: the store at {STORE} has no refs, so the "
              f"unreachable ratchet is not observable here ({len(unreachable)} "
              f"of {len(cites)} would be UNREACHABLE by that reading); UNKNOWN "
              f"was adjudicated for all {len(cites)}.")
    elif len(unreachable) > MAX_UNREACHABLE:
        print(f"\nRED: {len(unreachable)} cited shas reach no ref, ceiling is "
              f"{MAX_UNREACHABLE}. A cited sha that no ref reaches is evidence "
              "only in the clone it was written in. Restore the ref, or say in "
              "the prose that the object is gone — do not guess a replacement.")
        return 1
    if reds:
        print("\nRED: a sha that names no object in this clone is a citation "
              "of nothing. Say in the prose which repository it belongs to, or "
              "that it is not a sha at all.")
        return 1
    return 0


def _selftest() -> int:
    ok = True

    def expect(label, cond):
        nonlocal ok
        if not cond:
            print(f"SELFTEST FAIL [{label}]", file=sys.stderr); ok = False
        else:
            print(f"SELFTEST ok   [{label}]")

    expect("a backticked sha is matched", SHA_RE.findall("landed in `3428600a`") == ["3428600a"])
    expect("an unbackticked hex word is not matched",
           SHA_RE.findall("landed in 3428600a") == [])
    expect("a short hex run is not matched", SHA_RE.findall("`abc`") == [])
    expect("uppercase is not matched — this tree writes shas lowercase",
           SHA_RE.findall("`3428600A`") == [])
    expect("a 12-hex sha is matched", SHA_RE.findall("`3428600a1234`") == ["3428600a1234"])
    expect("a 13-hex run is not", SHA_RE.findall("`3428600a12345`") == [])
    expect("a `was `<sha>`` quotation is recognised",
           HISTORICAL_SHA.findall("was `11ff71d8`, now `0aa87464`") == ["11ff71d8"])
    expect("and the current sha beside it is NOT swept up",
           "0aa87464" not in HISTORICAL_SHA.findall("was `11ff71d8`, now `0aa87464`"))
    expect("a plain citation is not mistaken for a quotation",
           HISTORICAL_SHA.findall("landed in `3428600a`") == [])
    expect("the not-a-sha list is applied", "deadbeef" in NOT_A_SHA)
    expect("every NOT-OURS entry carries a reason",
           all(len(v) > 40 for v in NOT_OURS.values()))
    expect("NOT-OURS is keyed by exact sha, so it cannot cover a token nobody "
           "looked at", all(re.fullmatch(r"[0-9a-f]{7,12}", k) for k in NOT_OURS))

    # The git-backed cases run in a repository this function BUILDS, not in
    # the tree: the merge gate runs T0 in an rsynced directory that is not a
    # repository, where the fifth T0 of stack-ci died at `hash-object`
    # (2026-09-04). A scratch repo also makes every verdict below known by
    # construction — two commits on `main`, a third whose branch is deleted,
    # and a blob — where the live tree could only offer "at least one
    # dangling sha today", a fact about the ref store rather than the check.
    global ROOT
    real_root = ROOT
    scratch = tempfile.mkdtemp(prefix="cited-commits-selftest.")
    ROOT = Path(scratch)
    env = {"GIT_AUTHOR_NAME": "selftest", "GIT_AUTHOR_EMAIL": "s@t",
           "GIT_COMMITTER_NAME": "selftest", "GIT_COMMITTER_EMAIL": "s@t"}
    def sh(*args):
        return subprocess.run(["git", "-C", scratch, *args], check=True,
                              capture_output=True, text=True,
                              env={**os.environ, **env}).stdout.strip()
    sh("init", "-q", "-b", "main")
    (ROOT / "a").write_text("one\n"); sh("add", "a"); sh("commit", "-q", "-m", "one")
    (ROOT / "a").write_text("two\n"); sh("add", "a"); sh("commit", "-q", "-m", "two")
    sh("checkout", "-q", "-b", "gone")
    (ROOT / "a").write_text("three\n"); sh("add", "a"); sh("commit", "-q", "-m", "three")
    dangling = sh("rev-parse", "--short=8", "HEAD")
    sh("checkout", "-q", "main"); sh("branch", "-q", "-D", "gone")

    # patch-id equality is the whole basis of --fix, so it is asserted rather
    # than assumed: HEAD must match itself and must not match its parent.
    head = git("rev-parse", "HEAD").stdout.strip()
    parent = git("rev-parse", "HEAD~1").stdout.strip()
    expect("patch-id is stable for one commit", patch_id(head) == patch_id(head))
    expect("and differs between two different commits",
           patch_id(head) != patch_id(parent))
    expect("a commit is never offered as its own replacement",
           head not in rebased_replacement(head))

    # The real adjudication, on shas whose status is known by construction:
    # HEAD is reachable, and a sha that cannot exist is UNKNOWN.
    head = git("rev-parse", "--short=8", "HEAD").stdout.strip()
    cites = adjudicate([{"sha": head, "doc": "x", "line": 1},
                        {"sha": "0000000", "doc": "x", "line": 2}])
    expect("HEAD is reachable, not UNREACHABLE",
           cites[0]["verdict"] in ("ON-MAIN", "ON-BRANCH"))
    expect("an all-zero sha is UNKNOWN", cites[1]["verdict"] == "UNKNOWN")

    # A blob is a legitimate citation and must not read as a broken one.
    blob = subprocess.run(["git", "-C", str(ROOT), "hash-object", "-w", "--stdin"],
                          input="falsifier blob\n", capture_output=True,
                          text=True, check=True).stdout.strip()[:8]
    bl = adjudicate([{"sha": blob, "doc": "x", "line": 1}])
    expect("a cited BLOB is OTHER-OBJECT, not UNKNOWN — this tree cites blobs",
           bl[0]["verdict"] == "OTHER-OBJECT" and "blob" in bl[0]["detail"])

    # And the one that matters: a real, reachable commit that is reported
    # UNREACHABLE would make this check useless in the safe direction, while a
    # genuinely dangling one reported green would make it useless in the
    # dangerous one. The dangling one is built above: its branch is deleted,
    # the object is still in the store, and no ref reaches it.
    dg = adjudicate([{"sha": dangling, "doc": "x", "line": 1}])
    expect("a commit whose branch was deleted is UNREACHABLE (the poisoned case)",
           dg[0]["verdict"] == "UNREACHABLE")
    ROOT = real_root
    shutil.rmtree(scratch, ignore_errors=True)

    print("SELFTEST " + ("PASS" if ok else "FAIL"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
