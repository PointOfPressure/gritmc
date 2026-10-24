#!/usr/bin/env python3
"""Recover the commit a ledger figure was measured on, from the tree itself.

    python3 ci/figure-provenance.py                    # every figure lacking a sha
    python3 ci/figure-provenance.py bench-r7-rustmc-cpu
    python3 ci/figure-provenance.py --check            # exit 1 if any answer moved
    python3 ci/figure-provenance.py --snapshot         # stamp .figure-provenance for a gate

# WHY THIS EXISTS

`provenance` is the field that makes a figure the *before* of an attribution.
Twenty-five entries carried a bare date instead of a commit, and a date does not
identify a tree: `docs/BENCH.md` §3 could only attribute the stale-tenth drift
because someone had separately written down that the published r=7 baseline was
`ed0a29ec`, and every attribution whose baseline sha was missing stalled.

Twenty-four of the twenty-five entered the ledger in ONE commit — `5c5df53e`,
2026-08-07, the backfill that created this file over figures already published
in the documents. So the ledger's own history is worthless as evidence: it dates
the bookkeeping, not the measurement.

# THE DERIVATION, AND WHAT IT IS AND IS NOT

`ci/check-figures.py` rule 1 asserts that a figure's `claim` appears in its
`doc`. Run that rule *backwards* through history and it names the first commit
in which the claim was true — which is the commit that published the number, and
for this project's one-agent-one-measurement commits is the commit that took it.

That is an UPPER BOUND on the measuring tree and this tool says so rather than
pretending otherwise. A measurement is taken, then written up; the write-up
commit is at or after the run. Where the generator did not move between the two,
the bound is tight enough that the distinction cannot affect any number — which
is checkable, and `--paths` reports it.

A bare `claim` is not always a witness. `100.000000%` appears in this tree from
2026-07-22 and has nothing to do with `end-r16-n6-exact`. So the search uses the
narrowest token available, in order: `artefact_claim` (a full sentence with the
counts in it), then residual-and-denominator rendered with thousands separators,
then `claim`. The token actually used is printed, because a reader must be able
to see WHICH string the answer rests on.

Exit 0 fine, 1 `--check` found an answer that no longer matches the ledger.
"""

from __future__ import annotations

import hashlib
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "ci" / "lib"))
import box_env  # noqa: E402  (needs ROOT on the path first)

LEDGER = ROOT / "ci" / "figures.toml"
SHA = re.compile(r"\b[0-9a-f]{7,40}\b")

# The stamp a gate directory reads in place of the repository it does not have.
# Same idiom, same reason and the same `.gitignore` neighbourhood as
# `/.rp-commit`, which `ci/ow-r16-window*.sh` has stamped before shipping since
# 2026-08-06: a value only a tree with history can compute, written down by the
# tree that has it, for the tree that does not.
SNAPSHOT = ROOT / ".figure-provenance"
SNAPSHOT_VERSION = "1"

# One question, asked once. `main` used to ask it inline and `write_snapshot`
# needs the same answer, and two spellings of "is there a repository here" is
# how the two halves of a rule drift apart.
RESOLVE_SHAS = (
    subprocess.run(
        ["git", "-C", str(ROOT), "rev-parse", "--git-dir"], capture_output=True
    ).returncode
    == 0
)

# Set once in `main()`. Every `git()` call goes through it, so there is exactly
# one place that knows where this tree's object database is.
STORE: Path | None = None


def git(*args: str) -> str:
    """Ask this tree's object store. Empty output if it has none.

    # WHY THE EMPTY ANSWER USED TO BE A VERDICT

    This tool derives a figure's commit by running `ci/check-figures.py` rule 1
    **backwards** through history with `git log -S`. In a tree with no
    repository that command fails and returns nothing, and nothing was read as
    *the search found no commit* — so `--check` reported **56 of 56** figures
    as `NO WITNESS` and exit 1.

    `ci/t0.sh:126` calls this on every gate run, and every path that reaches
    `ci/t0.sh` ships the tree with `--exclude '.git'` first. So the blanket red
    was not a rare condition; it was the normal one, and the only reason it was
    not noticed for what it is is that it looks exactly like a real finding.

    `main()` now refuses to reach this function at all without a store. The
    guard here is belt and braces: a caller that imports this module and calls
    `git()` directly gets an empty string and no repository, which is the same
    silence, so the refusal has to live where the decision is made.
    """
    return subprocess.run(
        ["git", *box_env.store_args(STORE), *args],
        capture_output=True, text=True,
    ).stdout


def witnesses(fig: dict) -> list[tuple[str, str, str]]:
    """Every (path, token, kind) that could identify this figure, most specific first.

    ONE TOKEN IS NOT ENOUGH AND THE FAILURE IS INSTRUCTIVE. `bench-r7-rustmc-cpu`
    carries `artefact_claim = "BENCH_FLAVOURS"`, which is a variable name: search
    on it and the answer is `082f2370`, 2026-07-28, "rustmc is the slowest of the
    four" — an EARLIER bench run with different numbers. `end-r16-n6-exact`
    claims `100.000000%`, which this tree has carried since 2026-07-22 in
    unrelated contexts. A generic token does not fail loudly; it answers
    confidently and wrongly, which is the failure mode this whole ledger exists
    to catch.

    So this reports every witness and lets the reader see them agree. Where two
    independent tokens name the same commit that is a positive identification;
    where they disagree the disagreement is the finding and a human reads it.
    """
    out: list[tuple[str, str, str]] = []
    res, den = fig.get("residual"), fig.get("denominator")
    # The residual with thousands separators is the most specific thing a
    # figure owns: `317,725` is not a string that occurs by accident.
    if isinstance(res, int) and res >= 1000:
        out.append((fig["doc"], f"{res:,}", "residual"))
    if isinstance(den, int) and den >= 1000:
        out.append((fig["doc"], f"{den:,}", "denominator"))
    claim = fig["claim"].split("\n")[0].strip()
    if claim:
        out.append((fig["doc"], claim, "claim"))
    art, ac = fig.get("artefact"), fig.get("artefact_claim")
    if art and ac and (ROOT / art).exists():
        out.append((art, ac.split("\n")[0].strip(), "artefact_claim"))
    return out


def first_commit(doc: str, token: str) -> tuple[str, str, str] | None:
    """The earliest commit whose `doc` contains `token`."""
    out = git("log", "--format=%h|%ad|%s", "--date=short", "-S", token,
              "--reverse", "--", doc)
    rows = [l for l in out.strip().split("\n") if l]
    if not rows:
        return None
    h, d, s = rows[0].split("|", 2)
    return h, d, s


def full_oid(rev: str) -> str | None:
    """The 40-hex commit this abbreviation names, or `None` if it names none.

    **A PROVENANCE IS COMPARED AS AN OBJECT, NEVER AS A SUBSTRING — 2026-09-05.**
    `--check` used to ask `best not in prov`, where `best` is `git log
    --format=%h`'s abbreviation and `prov` is prose an agent wrote. Those are
    two different abbreviations of the same commit and git's is not a constant:
    it grows with the object count. Every one of the 56 entries this tool wrote
    carries an **8**-hex sha (74 candidates, all length 8, measured on main
    1a31e0a56) and `%h` in this repository now emits **9**, so
    `"165dc557f" in "... 165dc557 (2026-08-06) ..."` is false and the check
    reported **56 of 56 figures moved** on a ledger nothing had touched.

    That red was invisible for the reason the whole of rule 18 was: `--check`
    exits 0 without looking in a tree with no `.git`, which is every gate
    directory, so the only place it could speak is the only place nobody ran
    it. This file's own header said "Locally the same command is 56 re-derived,
    0 failing, exit 0" and that stopped being true when git widened the
    abbreviation; a comment recording a measurement is not the measurement.

    Resolving both sides removes the length from the question entirely: an
    abbreviation of any width names one commit or no commit.
    """
    out = subprocess.run(
        ["git", "-C", str(ROOT), "rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}"],
        capture_output=True,
        text=True,
    )
    oid = out.stdout.strip()
    return oid if out.returncode == 0 and len(oid) == 40 else None


def provenance_names(prov: str, oid: str) -> bool:
    """True when any hex-shaped run in `prov` resolves to the commit `oid`."""
    return any(full_oid(c) == oid for c in SHA.findall(prov))


def provenance_digest(prov: str) -> str:
    """What binds a snapshot row to the ledger line it was computed from.

    Without it the gate would be reading a verdict about a string it cannot
    check is the string it is holding, which is the shape of every check on
    this project that could not fail.
    """
    return hashlib.sha256(prov.encode("utf-8")).hexdigest()[:16]


def write_snapshot(path: Path) -> int:
    """Stamp every figure's provenance resolution for a tree that has no `.git`.

    # WHY A SNAPSHOT AND NOT THE REPOSITORY

    `ci/check-figures.py` rule 18 and this file's `--check` both need the object
    database, and `ci/t0-remote.sh` ships the tree **without** `.git` on purpose
    so a gate runs against uncommitted edits. So the rule that says "a
    provenance must name a commit that git resolves" has, since it was written,
    run only where it could not ask — degrading to shape in the gate and to
    `SKIPPED` here — while the same rule reads main RED on three figures in any
    worktree that has history. A rule enforced only where it is blind is this
    project's central defect class wearing a ledger's clothes.

    Measured before this file was changed, on main `1a31e0a56`: with history,
    `ci/check-figures.py` exits 1 with three `PROVENANCE-NO-COMMIT` reds
    (`bench-r32-rustmc-cpu` 48d1efb, `t0-floor-targets` 00ab0bde,
    `handoff-terminals-unforced` 917ebc43, none of which names an object
    anywhere on this box); in an rsync of the same tree with `.git` removed it
    prints `FIGURES GREEN` and exits 0.

    The two candidate repairs were shipping the repository and shipping the
    answer. `/home/sayer/grit/repo/.git` is **165 MB** against a snapshot of one
    line per figure, and shipping it would additionally give every gate
    directory a HEAD disagreeing with its own working tree, which is the
    property `--exclude .git` was chosen for. So: the answer travels, the
    database does not, and the enforcement point moves to the sync — which is
    the last place in the pipeline that still has history.

    Exit 2 rather than writing anything where there is no repository: a
    snapshot computed without an object database would say `UNRESOLVED` for
    every figure and read as a ledger fault, which is the exact confusion
    `--check` already refuses to make.
    """
    if not RESOLVE_SHAS:
        print(
            f"figure-provenance: REFUSED — {ROOT} is not inside a git repository, "
            "so no provenance can be resolved and a snapshot written here would "
            "be a row of UNRESOLVED that means only that. Stamp it in the "
            "worktree that has history, before the sync.",
            file=sys.stderr,
        )
        return 2
    figs = tomllib.loads(LEDGER.read_text())["figure"]
    head = full_oid("HEAD") or "unknown"
    rows = []
    unresolved = []
    for f in figs:
        prov = str(f.get("provenance", ""))
        oid = None
        for c in SHA.findall(prov):
            if box_env.looks_like_a_date(c):
                continue
            oid = full_oid(c)
            if oid:
                break
        if oid is None:
            unresolved.append(f["id"])
        rows.append(f"{f['id']}\t{provenance_digest(prov)}\t{oid or 'UNRESOLVED'}")
    body = [
        "# GENERATED by ci/figure-provenance.py --snapshot. Never committed; see",
        "# .gitignore. ci/check-figures.py rule 18 reads it in a tree with no .git.",
        f"version {SNAPSHOT_VERSION}",
        f"tree {head}",
        f"figures {len(rows)}",
        *rows,
    ]
    path.write_text("\n".join(body) + "\n", encoding="utf-8")
    print(
        f"figure-provenance: snapshot {path.name} — {len(rows)} figure(s), "
        f"{len(unresolved)} unresolved{': ' + ', '.join(unresolved) if unresolved else ''}"
    )
    # Unresolved entries are WRITTEN, not suppressed, and are still a non-zero
    # exit: the gate must be able to tell "checked, and this figure names no
    # commit" apart from "not checked", and a snapshot that quietly omitted the
    # failures would leave the gate reading green over a hole again.
    return 1 if unresolved else 0


def read_snapshot(path: Path) -> tuple[dict[str, tuple[str, str]], str | None]:
    """`{id: (provenance_digest, oid_or_UNRESOLVED)}`, or an error string."""
    if not path.exists():
        return {}, f"{path.name} is absent"
    rows: dict[str, tuple[str, str]] = {}
    version = None
    declared = None
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        if line.startswith("version "):
            version = line.split(None, 1)[1]
            continue
        if line.startswith("tree "):
            continue
        if line.startswith("figures "):
            declared = line.split(None, 1)[1]
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            return {}, f"{path.name} has a malformed row: {line[:60]!r}"
        rows[parts[0]] = (parts[1], parts[2])
    if version != SNAPSHOT_VERSION:
        return {}, f"{path.name} is version {version!r}, this checker reads {SNAPSHOT_VERSION!r}"
    if declared is None or int(declared) != len(rows):
        return {}, f"{path.name} declares {declared} figures and carries {len(rows)}"
    return rows, None


def moved(a: str, b: str, *paths: str) -> bool:
    """Did any of `paths` change between the two trees? A tight bound is one
    where the answer is no: the write-up commit is at or after the run, and if
    the generator did not move in between the distinction cannot reach a
    number."""
    return bool(git("diff", "--stat", a, b, "--", *paths).strip())


def _prov_names(prov: str, short: str) -> bool:
    """Does this provenance name the commit `short` identifies?

    # AN ABBREVIATION IS NOT AN IDENTITY, AND THIS COMPARED ABBREVIATIONS

    The test used to be `best not in prov` — a substring match of whatever
    `git log --format=%h` abbreviated to *today* against the text recorded
    whenever the figure was written. git chooses that length from the object
    count, so it grows as the repository does.

    It grew. On 2026-08-18 this tree passed the threshold from 8 characters to
    9, and every provenance recorded with an 8-character sha stopped matching a
    9-character derivation: **56 of 56 figures**, on pristine `origin/main`
    (`f82b2ebbf`) with a real `.git`, exit 1. Nothing about any figure changed.

    It was invisible because the repair that landed in `f82b2ebb` makes this
    check SKIP where there is no `.git`, and there is no `.git` in any gate
    directory — so the one environment that would have reported it is the one
    environment where the stage no longer runs. A skip that covers a genuine
    red is the failure mode a loud skip is supposed to prevent and does not,
    which is why `ci/t0-remote.sh` ships an object store instead.

    So: resolve the derived commit to its FULL sha, and ask whether any
    hex-shaped run in the provenance is a prefix of it. That is stable under
    the abbreviation length moving in either direction, and it is the question
    actually being asked — *does this prose name this commit?*
    """
    full = git("rev-parse", short).strip().lower()
    if not full:
        # No store to resolve against — a figure citing a commit that is not in
        # this object database, or a fixture. `short in prov` was here, and it
        # is the predicate that broke: it compares SPELLINGS, so the recorded
        # abbreviation and today's have to be the same length. `_tokens_name`
        # compares what the strings are ABOUT and is correct in both
        # directions, so the unresolvable case is no longer the one place the
        # regression can still live.
        return _tokens_name(prov, short)
    for cand in SHA.findall(prov.lower()):
        if full.startswith(cand):
            return True
    return False


def _tokens_name(prov: str, sha: str) -> bool:
    """Prefix-match in EITHER direction, for when no store can resolve `sha`.

    Neither side is required to have been written at today's abbreviation
    length, and neither is required to be the longer one. That second half is
    the part worth stating: an object store can also SHRINK — a fresh clone, a
    `git gc`, a shallow fetch, the bare store `ci/t0-remote.sh` ships — and only
    the growing direction has ever been observed here. A test that assumed the
    ledger is always the shorter side would pass today and fail on the first
    tree that was smaller than the one the figure was written on.

    From `ci-provenance-abbrev`, which reached this repair independently and was
    dropped rather than conflict with this branch.
    """
    for tok in SHA.findall(prov.lower()):
        if sha.lower().startswith(tok) or tok.startswith(sha.lower()):
            return True
    return False


def selftest() -> int:
    """Drive the provenance predicate, in both abbreviation directions and both arms.

    # WHY THIS IS DRIVEN AND NOT READ

    `--check` went from green to 56-of-56 red on `origin/main` without any
    figure being edited, because `git log --format=%h` abbreviates to whatever
    keeps a name unique in THIS object store and that length grew from 8 to 9
    on 2026-08-18. The predicate compared spellings, so every entry stopped
    matching at once.

    Nothing could have told anyone. That is the shape of the eight defects the
    gate-stage sweep found — the check worked, and nothing could say when it
    stopped working — so the predicate now has cases driven at it, and
    `ci/t0.sh` runs them BEFORE `--check`, so a comparison that has stopped
    comparing halts the gate instead of reporting green.

    # BOTH ARMS, BECAUSE ONLY ONE OF THEM RUNS IN ANGER

    `_prov_names` resolves the derived commit to its full sha and prefix-matches
    the recorded tokens against it. That arm needs an object store and is the
    arm every real gate takes. `_tokens_name` is the fallback for a sha this
    store cannot resolve, and matches prefixes in either direction.

    The five string cases below drive the FALLBACK only — their shas are
    synthetic and resolve nowhere. Driving just those would leave the arm that
    actually runs untested while reading green, which is this project's own
    defect class and would be a poor way to close an instance of it. So the
    resolved arm is driven separately against a real commit in this tree, and
    stands down loudly when there is no store rather than quietly not running.
    """
    long_, short = "a5f703d03", "a5f703d0"
    cases = [
        ("ledger 8, derived 9 — THE REGRESSION", f"{short} — measured here", long_, True),
        ("ledger 9, derived 8 — a SHRINKING store", f"{long_} — measured here", short, True),
        ("a different commit", f"{short} — measured here", "b0000000", False),
        ("shares a prefix and then diverges", "a5f703d0 — measured", "a5f70000000", False),
        ("no sha at all", "recovered by hand, 2026-08-02", long_, False),
    ]
    ok = True
    for name, prov, sha, want in cases:
        got = _tokens_name(prov, sha)
        if got != want:
            print(f"  FAIL  fallback: {name} -> {got}, wanted {want}")
            ok = False
        else:
            print(f"  ok    fallback: {name}")

    # -- the arm every gate actually takes -----------------------------------
    global STORE
    try:
        STORE = box_env.git_store(ROOT)
    except box_env.BoxEnvRed as exc:
        print(f"figure-provenance: {exc}", file=sys.stderr)
        return 1
    if STORE is None:
        box_env.skip(
            "figure-provenance:selftest-resolved",
            "no git object store — the five string cases above drove the "
            "fallback only, and the resolving arm that every gate takes was "
            "not exercised.",
        )
    else:
        head = git("rev-parse", "HEAD").strip()
        if len(head) < 40:
            print("  FAIL  resolved: could not read a full sha from the store")
            ok = False
        else:
            rcases = [
                ("ledger 8, derived full", head[:8], head, True),
                ("ledger 12, derived 9", head[:12], head[:9], True),
                ("ledger full, derived 8 — a SHRINKING store", head, head[:8], True),
                ("a different commit", "b" * 8 if head[0] != "b" else "c" * 8, head, False),
            ]
            for name, recorded, derived, want in rcases:
                got = _prov_names(f"branch x off y {recorded}, 2026-08-18", derived)
                if got != want:
                    print(f"  FAIL  resolved: {name} -> {got}, wanted {want}")
                    ok = False
                else:
                    print(f"  ok    resolved: {name}")

    print("figure-provenance selftest: PASS" if ok else
          "figure-provenance selftest: FAIL")
    return 0 if ok else 1


def main(argv: list[str]) -> int:
    global STORE
    if "--selftest" in argv:
        return selftest()
    check = "--check" in argv
    want = [a for a in argv if not a.startswith("-")]

    if "--snapshot" in argv:
        return write_snapshot(Path(want[0]) if want else SNAPSHOT)

    # SUPERSEDES THE SKIP-PROBE THAT LANDED ON MAIN IN `f82b2ebb`. That one
    # asks `rev-parse --git-dir` and stands down when there is no repository.
    # This one asks `ci/lib/box_env.py`, which also finds the bare object store
    # `ci/t0-remote.sh` now ships — so in a gate directory the 56 figures are
    # actually re-derived rather than skipped, and the skip covers only a tree
    # somebody assembled by hand. It also goes through `box_env.skip`, so
    # `ci/box-skip-audit.py` counts it; a `print` reaches the log and not the
    # ledger the audit reads.
    # THE DERIVATION IS A HISTORY SEARCH AND CANNOT BE FAKED FROM A CACHE.
    #
    # `git log -S<token>` walks every commit that touched the path. There is no
    # summary of that a `.git`-less tree could carry which would not simply be
    # this tool's own previous answer written down — and a check that verifies
    # its own cached output is the unfalsifiable class this project keeps
    # finding. So the choice is a real object store or an honest absence, and
    # `ci/t0-remote.sh` now ships the store.
    try:
        STORE = box_env.git_store(ROOT)
    except box_env.BoxEnvRed as exc:
        print(f"figure-provenance: {exc}", file=sys.stderr)
        return 1
    if STORE is None:
        box_env.skip(
            "figure-provenance",
            f"no git object store in {ROOT} — no figure's commit was "
            f"re-derived. Ship one with ci/t0-remote.sh, or set "
            f"RUSTMC_GIT_DIR.",
        )
        return 0

    figs = tomllib.loads(LEDGER.read_text())["figure"]
    if want:
        figs = [f for f in figs if f["id"] in want]
    elif check:
        # `--check` RE-DERIVES WHAT THIS TOOL RECOVERED, and the selector is
        # the interesting part.
        #
        # Wiring it to the default one — entries with no sha — made it green
        # over an EMPTY SET the moment the last provenance was filled in: a
        # check that could not fail, inside a script written to close a
        # different instance of exactly that. Wiring it to *every* figure is
        # the opposite error and was measured rather than assumed: 43 of 103
        # go red, because a provenance written by hand often names the branch's
        # BASE (`branch X off Y 27914806`) while the derivation names the
        # commit that first published the claim, and both are right about
        # different things.
        #
        # So the set is the entries this tool wrote, marked by its own name in
        # the provenance. Those are the ones whose sha IS the derived one by
        # construction, so a disagreement means the derivation moved — the
        # document was edited, or the claim string changed — and that is
        # exactly what wants catching. An empty set is refused below.
        figs = [f for f in figs if "figure-provenance.py" in f.get("provenance", "")]
    else:
        figs = [f for f in figs if not SHA.search(f.get("provenance", ""))]

    bad = 0
    for f in figs:
        prov = f.get("provenance", "")
        recorded = re.search(r"\d{4}-\d{2}-\d{2}", prov)
        hits: dict[str, list[str]] = {}
        rows = []
        for path, tok, kind in witnesses(f):
            hit = first_commit(path, tok)
            if hit is None:
                rows.append(f"      {kind:<15} {'(no match)':<24} {tok[:44]!r}")
                continue
            h, d, s_ = hit
            hits.setdefault(h, []).append(kind)
            rows.append(f"      {kind:<15} {h} {d}  {s_[:60]}")
        if not hits:
            print(f"{f['id']:<32} NO WITNESS")
            print(f"MISMATCH {f['id']}: no witness found in history")
            bad += 1
            continue
        # The answer is the commit the MOST tokens agree on; ties break toward
        # the most specific token, which is the order `witnesses` returns.
        best = max(hits, key=lambda h: (len(hits[h]), -list(hits).index(h)))
        agree = len(hits[best])
        verdict = "AGREED" if agree > 1 else "single"
        note = ""
        d = [r for r in rows if best in r][0].split()[2]
        if recorded and recorded.group(0) != d:
            note = f"  [ledger date {recorded.group(0)}]"
        print(f"{f['id']:<32} {best}  {d}  {verdict} on {agree}/{len(rows)} "
              f"witness(es): {','.join(hits[best])}{note}")
        for r in rows:
            print(r)
        if check and not _prov_names(prov, best):
            bad += 1
            print(f"MISMATCH {f['id']}: derived {best} is not named in its provenance")
    if check:
        print(f"\nfigure-provenance: {len(figs)} figure(s) re-derived, {bad} whose "
              "derived commit is not in their provenance")
        if not figs:
            print("figure-provenance: nothing to check — refusing to report "
                  "green over an empty set", file=sys.stderr)
            return 2
    return 1 if (check and bad) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
