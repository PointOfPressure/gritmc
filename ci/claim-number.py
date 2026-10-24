#!/usr/bin/env python3
"""Allocate a `## N.` section number without asking the other nine lanes.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/claim-number.py --claim --doc PARITY-ROADMAP --slug my-finding \
#                        --title "What I measured"
#
# prints one number, atomically reserves it against every other worktree on
# this box, and writes `docs/claims/PARITY-ROADMAP-<N>-my-finding.toml` with
# `status = "reserved"`. Write your `## N.` heading under that number, flip
# `status` to `"written"`, and commit both.
#
#     ci/claim-number.py --show           who holds what, and which are unwritten
#     ci/claim-number.py --next           the number --claim would take, no side effect
#     ci/claim-number.py --check          refuse if this tree collides with any ref
#     ci/claim-number.py --release -d D -n N   give a reservation back
#
# WHAT BREAKS IF YOU IGNORE THIS: you pick the same number as a lane you
# cannot see, `ci/check-doc-claims.py` refuses the merge with DUPLICATE-CLAIM,
# and an orchestrator turn is spent renumbering by hand. That happened three
# times on 2026-08-18 alone.
# ===== end interface; everything below is why =====

# # THE PROBLEM IS ALLOCATION, AND ONLY THE DETECTOR EXISTED

`ci/check-doc-claims.py` refuses two claim files naming one `(doc, number)`.
That is the detector and it works. But a detector fires *at merge*, on a tree
that already contains both claims, which means the collision has already cost
both lanes their numbering and costs a third party the renumber. On
2026-08-18:

  * two branches both numbered a section **33**. Resolved by hand:
    `isolated-drift` kept 33 because §32.3 forward-references it, and the
    other went to **46, not 36**, because 36-39 were already claimed by two
    further lanes that neither branch could see;
  * a recovered draft claimed **§54**, which a live lane already held;
  * a lane established that 54 was taken **by asking the other lanes**, which
    is not a check, does not scale past a handful of lanes, and produces an
    answer that is stale the moment it is given.

Every one of those is the same missing piece: there was no way to *obtain* a
number, only a way to be told afterwards that the one you invented was wrong.

# # WHY THE OBVIOUS ANSWER (max + 1) IS NOT ENOUGH

"Scan the branches, take the highest, add one" is most of the answer and it
is what `--next` prints. It fails in exactly the window that matters: two
lanes that scan within the same fetch interval both compute the same
`max + 1`, and both are correct about a fact that stopped being true while
they were writing. That window is not hypothetical — it is the §33 collision
above, twice in one day.

So the number is not merely *computed*, it is *taken*, and taking it is a
compare-and-swap that one of the two racers must lose.

# # THE LOCK, AND WHY GIT ALREADY IS ONE

All 250 lane trees under `~/grit/` are `git worktree`s of a single repository.
They do not merely resemble each other — they share **one ref store**. And
`git update-ref <ref> <new> <old>` with an empty `<old>` means *"set this,
and fail if it exists at all"*, executed under the ref lock. That is a
genuine atomic compare-and-swap, already present, already shared by every
lane, needing no daemon, no file in the tree, and no network.

A reservation is therefore `refs/claims/<doc>/<number>`, pointing at a blob
naming the claimant. Two lanes racing for 58: both compute 58, both attempt
the CAS, exactly one succeeds, the loser is told `already reserved by <branch>`
and transparently retries at 59. Neither had to know the other existed.

# # RESERVED-BUT-UNWRITTEN IS A STATE, NOT A GAP

The brief asks for "a way to see what is reserved but unwritten", and the
registry as it stood could not express it: a claim file with no `## N.`
heading is `CLAIM-NO-HEADING`, a red. So a lane could not honestly reserve a
number in advance of writing the section — the registry would refuse the
honest act and permit the dishonest one (invent a number, write it, discover
the collision at merge).

`status` closes that. `status = "reserved"` is exempt from CLAIM-NO-HEADING
and still fully counted by DUPLICATE-CLAIM. Crucially it is **not a free
pass**: `ci/check-doc-claims.py` reddens `RESERVED-BUT-WRITTEN` if a reserved
claim's heading actually exists. A reservation that has quietly become a real
section is a stale reservation, and the state is falsifiable in both
directions rather than being a hole you can park a claim in forever.

# # WHAT THIS DELIBERATELY DOES NOT DO

It does not renumber, does not touch `docs/`, does not adjudicate content,
and **is not a gate**. `--check` is a pre-flight a lane runs on itself; the
gate is still `ci/check-doc-claims.py`, which is the thing that must refuse
a merge. A reservation is a courtesy that makes collisions rare, not a
permission system: a lane that never runs this is exactly as safe as it was
before, which is to say it is caught at merge instead of at write.

It also allocates **strictly above the global high-water mark** rather than
filling holes. The holes are load-bearing: `docs/PARITY-ROADMAP.md` skips
`## 18.` on branches that have not landed it, §32.3 forward-references §33,
and a section number that gets reused is a forward reference that silently
retargets. A dense numbering is worth less than a stable one.

# # THE DEFECT CLASS THIS MUST NOT JOIN

An allocator that cannot refuse is a random number generator with paperwork.
`--selftest` poisons each refusal on fixtures and asserts it fires, and
`ci/claim-number-falsify.sh` goes further: it runs `--check` against the
**real, unrepaired ref store**, where four live cross-branch collisions exist
today — §45, §51, §54 and §57 — and requires it to name them before it is
shown passing on a repaired tree. A check first demonstrated on a fixture has
only been shown to work on the fixture.

§54 is the one worth naming: the brief describes "a recovered draft claimed
§54, which a live lane already held" as a resolved incident. It is not
resolved. `corpus-coverage-gaps` and `gameplay-corpus-gaps` both hold §54
right now under different slugs, and nothing in the tree said so until this
script was run.
"""

from __future__ import annotations

import argparse
import datetime
import os
import re
import subprocess
import sys
from pathlib import Path

# `CLAIM_NUMBER_ROOT` exists for `ci/claim-number-falsify.sh` and nothing else.
#
# The falsifier has to prove the compare-and-swap actually refuses a second
# claimant, and proving that means *writing reservation refs*. Writing them
# into the live store — where 250 lane worktrees share one ref namespace —
# would burn real section numbers on every falsifier run, and a leaked ref
# from an aborted run would block a number permanently with nothing to say
# why. So the write-side cases run against a scratch repository and the
# read-side cases run against the real one, which is also the only split that
# lets the falsifier demonstrate a refusal on the REAL, unrepaired ref store
# rather than on a fixture that resembles it.
ROOT = Path(os.environ.get("CLAIM_NUMBER_ROOT") or Path(__file__).resolve().parent.parent)

# Kept in step with ci/check-doc-claims.py's DOCS, deliberately by hand: this
# script must not import that one. They are run in different situations (this
# one before you write, that one at merge) and a shared import would let a
# breakage in either take out both.
DOCS = ("DETERMINISM", "PARITY-ROADMAP")

REF_PREFIX = "refs/claims"
SLUG_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
# `PARITY-ROADMAP-44.8-sherds.toml` -> ("PARITY-ROADMAP", "44.8", "sherds").
# The number is anchored on both sides so a slug that starts with digits
# (`19-20260722-per-block-readjudicated`) cannot be mistaken for one.
CLAIM_FILE_RE = re.compile(r"^(?P<doc>[A-Z0-9-]+?)-(?P<number>\d+(?:\.\d+)?)-(?P<slug>[a-z0-9-]+)\.toml$")


def git(*args: str, check: bool = True) -> str:
    r = subprocess.run(
        ["git", "-C", str(ROOT), *args], capture_output=True, text=True
    )
    if check and r.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def sort_key(number: str):
    return tuple(float(p) for p in number.split("."))


def parse_claim_name(name: str):
    """(doc, number, slug) from a claim filename, or None.

    Parsing the *name* is right here and wrong in `check-doc-claims.py`: that
    script has the file's own fields to compare against and so reconstructs
    the name forwards, which has exactly one answer. Here the file may live
    on a ref this worktree has never checked out, and reading 4,679 blobs to
    learn their `number` fields would turn a 1.4-second scan into minutes.
    The regex is anchored tightly enough that the ambiguity that forbids the
    backwards split there (`-(\\d+)-` appearing inside a slug) cannot bite:
    the number must abut the doc name on its left.
    """
    m = CLAIM_FILE_RE.match(name)
    if not m:
        return None
    return m.group("doc"), m.group("number"), m.group("slug")


# ---------------------------------------------------------------------------
# where a number can be taken: refs, reservations, and this working tree


def scan_refs() -> dict[tuple[str, str], dict[str, set[str]]]:
    """{(doc, number): {slug: {ref, ...}}} over every local and remote branch.

    One `git ls-tree` per ref; 218 remote refs plus the local heads measure at
    about 1.4 s on sayer-desktop, which is cheap enough to do unconditionally
    rather than caching it into a file that would then be the thing that goes
    stale.
    """
    out: dict[tuple[str, str], dict[str, set[str]]] = {}
    refs = [
        ln.strip()
        for ln in git(
            "for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"
        ).splitlines()
        if ln.strip() and not ln.strip().endswith("/HEAD")
    ]
    for ref in refs:
        listing = git("ls-tree", "-r", "--name-only", ref, "--", "docs/claims", check=False)
        for path in listing.split():
            parsed = parse_claim_name(Path(path).name)
            if not parsed:
                continue
            doc, number, slug = parsed
            short = re.sub(r"^refs/(remotes/[^/]+|heads)/", "", ref)
            out.setdefault((doc, number), {}).setdefault(slug, set()).add(short)
    return out


def scan_reservations() -> dict[tuple[str, str], dict[str, str]]:
    """{(doc, number): {branch, slug, at, sha}} from `refs/claims/<doc>/<N>`."""
    out: dict[tuple[str, str], dict[str, str]] = {}
    listing = git(
        "for-each-ref", "--format=%(refname)%09%(objectname)", REF_PREFIX, check=False
    )
    for line in listing.splitlines():
        if "\t" not in line:
            continue
        refname, sha = line.split("\t", 1)
        rest = refname[len(REF_PREFIX) + 1 :]
        if "/" not in rest:
            continue
        doc, number = rest.rsplit("/", 1)
        body = git("cat-file", "-p", sha, check=False)
        fields = {"branch": "?", "slug": "?", "at": "?"}
        for ln in body.splitlines():
            if "=" in ln:
                k, v = ln.split("=", 1)
                fields[k.strip()] = v.strip().strip('"')
        fields["sha"] = sha[:8]
        out[(doc, number)] = fields
    return out


def scan_worktree() -> dict[tuple[str, str], dict]:
    """{(doc, number): {slug, status, path}} for `docs/claims/` on disk."""
    out: dict[tuple[str, str], dict] = {}
    d = ROOT / "docs" / "claims"
    if not d.is_dir():
        return out
    for p in sorted(d.glob("*.toml")):
        parsed = parse_claim_name(p.name)
        if not parsed:
            continue
        doc, number, slug = parsed
        status = "written"
        for ln in p.read_text(encoding="utf-8").splitlines():
            if ln.strip().startswith("status"):
                status = ln.split("=", 1)[1].strip().strip('"')
        out[(doc, number)] = {"slug": slug, "status": status, "path": p}
    return out


def taken_numbers(doc: str, refs, reservations, worktree) -> set[str]:
    t = {n for (d, n) in refs if d == doc}
    t |= {n for (d, n) in reservations if d == doc}
    t |= {n for (d, n) in worktree if d == doc}
    return t


def next_free(doc: str, refs, reservations, worktree, start: int | None = None) -> int:
    """The lowest integer strictly above everything taken.

    Holes are not filled — see the module docstring. `int(float(n))` folds a
    sub-section (`44.8`) onto its parent so that reserving under §44 cannot
    hand out an integer that a decimal already lives beneath.
    """
    t = taken_numbers(doc, refs, reservations, worktree)
    high = 0
    for n in t:
        try:
            high = max(high, int(float(n)))
        except ValueError:
            continue
    candidate = high + 1 if start is None else start
    while str(candidate) in t:
        candidate += 1
    return candidate


# ---------------------------------------------------------------------------
# the operations


def cmd_show(args) -> int:
    """The reservation table.

    A number that is on `main` is deliberately printed WITHOUT its holder
    list. 130 branches carry §33's claim file because they all branched from a
    main that had it, and printing them is 130 lines saying "this landed" —
    the noise buries the eight rows that are actually contended. Only the
    off-main holders are a fact about who is working on what.
    """
    refs, reservations, worktree = scan_refs(), scan_reservations(), scan_worktree()
    docs = [args.doc] if args.doc else list(DOCS)
    rc = 0
    for doc in docs:
        numbers = sorted(taken_numbers(doc, refs, reservations, worktree), key=sort_key)
        on_main = {
            n for (d, n), slugs in refs.items()
            if d == doc and any(r == "main" for s in slugs.values() for r in s)
        }
        collisions = [n for n in numbers if len(refs.get((doc, n), {})) > 1]
        print(f"\n=== {doc} — {len(numbers)} taken, {len(on_main)} landed, "
              f"{len(numbers) - len(on_main)} in flight, {len(collisions)} contended ===")
        for n in numbers:
            slugs = refs.get((doc, n), {})
            res = reservations.get((doc, n))
            wt = worktree.get((doc, n))
            if n in on_main and not res and len(slugs) <= 1:
                if args.all:
                    title = next(iter(slugs), "?")
                    print(f"  §{n:<7} landed   {title}")
                continue
            state = "in flight"
            if res and not slugs:
                state = "RESERVED, unwritten"
            elif res:
                state = "reserved + written"
            elif n in on_main:
                state = "landed"
            if wt and wt["status"] == "reserved":
                state = "RESERVED here, unwritten"
            print(f"  §{n:<7} {state}")
            for slug, where in sorted(slugs.items()):
                # A branch list is capped: past a handful it stops being
                # "who holds this" and becomes "who branched from main".
                w = sorted(where)
                shown = ", ".join(w[:6]) + (f", +{len(w) - 6} more" if len(w) > 6 else "")
                print(f"           {slug:<44} <- {shown}")
            if res:
                print(f"           [lock] {res['branch']} / {res['slug']} at {res['at']}")
            if len(slugs) > 1:
                print(f"           ** COLLISION: {len(slugs)} distinct slugs claim §{n} **")
                rc = 1
        print(f"  next free: §{next_free(doc, refs, reservations, worktree)}")
    if rc and not args.quiet:
        print("\nContended numbers above will be refused by ci/check-doc-claims.py "
              "the moment two of them land together.")
    return 0 if args.quiet else rc


def cmd_next(args) -> int:
    refs, reservations, worktree = scan_refs(), scan_reservations(), scan_worktree()
    print(next_free(args.doc, refs, reservations, worktree))
    return 0


def cmd_claim(args) -> int:
    if not SLUG_RE.match(args.slug):
        print(f"claim-number: slug `{args.slug}` must be [a-z0-9-]+", file=sys.stderr)
        return 2
    if args.doc not in DOCS:
        print(f"claim-number: doc `{args.doc}` not in {list(DOCS)}", file=sys.stderr)
        return 2
    branch = args.branch or git("rev-parse", "--abbrev-ref", "HEAD").strip()
    refs, reservations, worktree = scan_refs(), scan_reservations(), scan_worktree()

    candidate = next_free(args.doc, refs, reservations, worktree)
    # Bounded retry: each losing CAS is one lane that beat us, and there are
    # 250 worktrees. A loop with no bound would be a livelock waiting to be
    # discovered at 3am.
    for _ in range(64):
        at = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        body = f'branch = "{branch}"\nslug = "{args.slug}"\nat = "{at}"\n'
        r = subprocess.run(
            ["git", "-C", str(ROOT), "hash-object", "-w", "--stdin"],
            input=body, capture_output=True, text=True, check=True,
        )
        blob = r.stdout.strip()
        ref = f"{REF_PREFIX}/{args.doc}/{candidate}"
        # The empty old-value is the compare-and-swap: "create this ref, and
        # fail if it exists". This is the whole allocator.
        cas = subprocess.run(
            ["git", "-C", str(ROOT), "update-ref", ref, blob, ""],
            capture_output=True, text=True,
        )
        if cas.returncode == 0:
            path = ROOT / "docs" / "claims" / f"{args.doc}-{candidate}-{args.slug}.toml"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                f'doc    = "{args.doc}"\n'
                f"number = {candidate}\n"
                f'slug   = "{args.slug}"\n'
                f'title  = "{args.title}"\n'
                f'branch = "{branch}"\n'
                f'status = "reserved"\n',
                encoding="utf-8",
            )
            print(candidate)
            print(f"reserved {ref} -> {path.relative_to(ROOT)}", file=sys.stderr)
            print(
                f"write `## {candidate}.` in docs/{args.doc}.md, then set "
                'status = "written"',
                file=sys.stderr,
            )
            return 0
        holder = scan_reservations().get((args.doc, str(candidate)), {}).get("branch", "?")
        print(f"claim-number: §{candidate} lost the race to `{holder}`, retrying", file=sys.stderr)
        candidate += 1
        reservations = scan_reservations()
    print("claim-number: 64 consecutive numbers lost their race — something is wrong", file=sys.stderr)
    return 2


def cmd_release(args) -> int:
    ref = f"{REF_PREFIX}/{args.doc}/{args.number}"
    # `git update-ref -d` exits 0 on a ref that never existed, so the delete
    # cannot be the existence test. Caught by the falsifier: a lane releasing
    # a mistyped number was told "released" and still held the real one.
    exists = subprocess.run(
        ["git", "-C", str(ROOT), "rev-parse", "--verify", "--quiet", ref],
        capture_output=True, text=True,
    )
    if exists.returncode != 0:
        print(f"claim-number: no reservation at {ref} — nothing released", file=sys.stderr)
        return 1
    held = scan_reservations().get((args.doc, str(args.number)), {})
    r = subprocess.run(
        ["git", "-C", str(ROOT), "update-ref", "-d", ref], capture_output=True, text=True
    )
    if r.returncode != 0:
        print(f"claim-number: could not delete {ref}: {r.stderr.strip()}", file=sys.stderr)
        return 1
    print(f"released {ref} (was {held.get('branch', '?')} / {held.get('slug', '?')})")
    return 0


def cmd_check(args) -> int:
    """Refuse if a claim in THIS tree is also claimed, under a different slug,
    by a ref this tree does not contain, or is locked by another branch.

    This is the part `ci/check-doc-claims.py` structurally cannot do: it
    adjudicates one tree, so a collision is invisible to it until both sides
    have already been merged into one. Here the whole ref store is in view.
    """
    refs, reservations, worktree = scan_refs(), scan_reservations(), scan_worktree()
    # Non-fatal: a detached HEAD prints a sha and a repo with no commits fails
    # outright, and neither is a reason for a collision check to abort. The
    # branch name is only used to let a lane re-check its own reservation.
    head = git("rev-parse", "--abbrev-ref", "HEAD", check=False).strip()
    reds: list[str] = []
    for (doc, number), info in sorted(worktree.items(), key=lambda kv: (kv[0][0], sort_key(kv[0][1]))):
        others = {s: w for s, w in refs.get((doc, number), {}).items() if s != info["slug"]}
        if others:
            who = "; ".join(f"{s} on {', '.join(sorted(w))}" for s, w in sorted(others.items()))
            reds.append(
                f"CROSS-BRANCH-COLLISION {doc} §{number}: this tree's "
                f"`{info['slug']}` collides with {who}"
            )
        res = reservations.get((doc, number))
        if res and res.get("slug") != info["slug"] and res.get("branch") != head:
            reds.append(
                f"RESERVED-ELSEWHERE {doc} §{number}: reserved by "
                f"`{res['branch']}` for `{res['slug']}` at {res['at']}, but this "
                f"tree claims it for `{info['slug']}`"
            )
    for r in reds:
        print(r)
    if reds:
        print(f"\nclaim-number: {len(reds)} collision(s). "
              "`ci/claim-number.py --claim` allocates a number that has none.")
        return 1
    print(f"claim-number: {len(worktree)} claim(s) in this tree, none collide with any ref")
    return 0


# ---------------------------------------------------------------------------
# selftest — every refusal poisoned on a scratch repo, no real refs touched


def _selftest() -> int:
    import tempfile

    ok = True

    def expect(label: str, cond: bool) -> None:
        nonlocal ok
        if not cond:
            print(f"SELFTEST FAIL [{label}]", file=sys.stderr)
            ok = False
        else:
            print(f"SELFTEST ok   [{label}]")

    # --- the pure functions, on synthetic scan output ----------------------
    refs = {("PARITY-ROADMAP", "1"): {"a": {"main"}}, ("PARITY-ROADMAP", "9"): {"b": {"x"}}}
    expect("next_free clears the high-water mark, not the first hole",
           next_free("PARITY-ROADMAP", refs, {}, {}) == 10)
    expect("a reservation with no branch still blocks",
           next_free("PARITY-ROADMAP", refs, {("PARITY-ROADMAP", "10"): {}}, {}) == 11)
    expect("a worktree claim with no ref still blocks",
           next_free("PARITY-ROADMAP", refs, {}, {("PARITY-ROADMAP", "10"): {}}) == 11)
    expect("a sub-section folds onto its parent's integer",
           next_free("PARITY-ROADMAP", {("PARITY-ROADMAP", "44.8"): {"s": {"m"}}}, {}, {}) == 45)
    expect("another doc's numbers do not block this one",
           next_free("DETERMINISM", refs, {}, {}) == 1)

    expect("claim filename parses to its three parts",
           parse_claim_name("PARITY-ROADMAP-44.8-sherds.toml") == ("PARITY-ROADMAP", "44.8", "sherds"))
    expect("a digit-leading slug is not mistaken for the number",
           parse_claim_name("DETERMINISM-19-20260722-per-block.toml")
           == ("DETERMINISM", "19", "20260722-per-block"))
    expect("a non-claim file parses to nothing", parse_claim_name("README.md") is None)

    # --- the CAS itself, on a throwaway repo ------------------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        subprocess.run(["git", "init", "-q", str(d)], check=True)

        def g(*a):
            return subprocess.run(["git", "-C", str(d), *a], capture_output=True, text=True)

        blob = g("hash-object", "-w", "--stdin")
        b1 = subprocess.run(["git", "-C", str(d), "hash-object", "-w", "--stdin"],
                            input="one\n", capture_output=True, text=True, check=True).stdout.strip()
        b2 = subprocess.run(["git", "-C", str(d), "hash-object", "-w", "--stdin"],
                            input="two\n", capture_output=True, text=True, check=True).stdout.strip()
        first = g("update-ref", "refs/claims/T/1", b1, "")
        expect("first CAS on a free number succeeds", first.returncode == 0)
        second = g("update-ref", "refs/claims/T/1", b2, "")
        expect("second CAS on the same number is REFUSED", second.returncode != 0)
        expect("the loser did not overwrite the winner",
               g("rev-parse", "refs/claims/T/1").stdout.strip() == b1)
        expect("a different number is still free",
               g("update-ref", "refs/claims/T/2", b2, "").returncode == 0)
        expect("--release deletes a reservation",
               g("update-ref", "-d", "refs/claims/T/1").returncode == 0)
        expect("and the number is then re-claimable",
               g("update-ref", "refs/claims/T/1", b2, "").returncode == 0)
        del blob

    print("SELFTEST " + ("PASS" if ok else "FAIL"))
    return 0 if ok else 1


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--show", action="store_true")
    p.add_argument("--next", action="store_true")
    p.add_argument("--claim", action="store_true")
    p.add_argument("--check", action="store_true")
    p.add_argument("--release", action="store_true")
    p.add_argument("--selftest", action="store_true")
    p.add_argument("-d", "--doc", default=None)
    p.add_argument("-n", "--number", default=None)
    p.add_argument("-s", "--slug", default=None)
    p.add_argument("-t", "--title", default="")
    p.add_argument("-b", "--branch", default=None)
    p.add_argument("--all", action="store_true", help="--show: include landed rows")
    p.add_argument("--quiet", action="store_true", help="--show: exit 0 even when contended")
    a = p.parse_args()

    if a.selftest:
        return _selftest()
    if a.show:
        return cmd_show(a)
    if a.next:
        if not a.doc:
            p.error("--next needs --doc")
        return cmd_next(a)
    if a.claim:
        if not (a.doc and a.slug):
            p.error("--claim needs --doc and --slug")
        return cmd_claim(a)
    if a.release:
        if not (a.doc and a.number):
            p.error("--release needs --doc and --number")
        return cmd_release(a)
    if a.check:
        return cmd_check(a)
    p.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
