#!/usr/bin/env python3
"""Git merge driver for a genuinely append-only doc: `docs/DETERMINISM.md`.

# WHY THIS EXISTS

`docs/DETERMINISM.md` is append-only with hand-numbered `## N.` sections.
Two branches that each append land their new section at the same anchor —
the end of the file — so an ordinary line-based 3-way merge sees both sides
touching the same region and conflicts, even though the two edits do not
overlap in any way that matters: neither branch read or depended on the
other's tail. `git merge -X ours`/`-X theirs` would silently drop one side;
`merge=union` (deliberately **not** used here — see `docs/AGENT-BRIEF.md` and
the note on `docs/WORKLIST.md`, which must never get this driver) can
interleave lines from both sides inside a single evolving paragraph and has
already produced a real defect that way. For a file where every edit is a
whole new tail appended after the last one, the correct 3-way answer is:
keep the base, then keep **both** tails, each internally in the order its
own branch wrote it.

# THE PROTOCOL

Invoked by git through the `merge.appendonly.driver` config value
(`ci/setup-merge-drivers.sh` sets it) as:

    python3 ci/merge-appendonly.py %O %A %B

`%O` is the common ancestor, `%A` is our side, `%B` is their side. Git has
already written all three to those paths before calling this. The merged
result must be written back into `%A` — that is the path git reads the
result from regardless of which side "won" — and the exit code says whether
the merge was clean (`0`) or a real conflict (non-zero).

# WHAT MAKES THIS SAFE RATHER THAN A KEEP-BOTH HACK

**It only takes the whole-file fast path when the append-only assumption
actually holds**: both `%A` and `%B` must be exact byte-prefix extensions of
`%O`. If either side has touched anything in the base region — not just
appended after it — concatenating the two files whole would silently discard
or duplicate an edit, so that shortcut is not taken.

# THE CLAIM THIS FILE USED TO MAKE, AND WHAT IT COST — 2026-08-17

It used to end: *"This driver only ever makes the append case cheaper; it
never makes the non-append case worse."* **That was false, and it was the
single largest thing blocking the worldgen branches.** When the byte-prefix
precondition failed, this driver wrote *the whole of each side* between
conflict markers — so a merge git's own driver would have resolved into one
small hunk, or into nothing at all, came back as a conflict spanning the
file. Measured on `docs/DETERMINISM.md` (7,191 lines on `main` that morning):

* `clamped-floor-probe` first commit — driver: one conflict hunk **7,318
  lines** long. `git merge-file --diff3` on the same three inputs: **one hunk
  of 167 lines**, and the two sides' edits never touched each other.
* `worldgen-sulfur-r18` — driver: **51,298 lines** in one hunk.
* Six of the seven `REBASE-CONFLICT` rows in that morning's `preflight.tsv`
  were worldgen branches, and **five of the six conflicted only here**. None
  of the five had a real disagreement with `main` about any existing line.

A conflict nobody can read is not "the same as before this driver existed" —
before it existed, git produced a legible hunk. So the refusal path is now a
real merge:

1. **`git merge-file --diff3`**, git's own line-level 3-way, on the same three
   inputs. If that is clean, the merge is clean.
2. Otherwise the append-only rule is applied **per hunk instead of per file**:
   a hunk whose *base* side is empty is both branches adding different text at
   the same anchor — the exact case this driver exists for, just not spanning
   the whole file — and it is resolved ours-then-theirs, in commit order.
   A hunk where one side left the base region **byte-identical** and added
   text next to it, while the other side edited the region, is the same thing
   wearing adjacency: git lumps the two edits into one hunk because they
   touch, not because they disagree. The addition is kept and the edit taken.
   Neither rule fires when the side replacing the base region is empty or
   blank — that is a deletion, the one edit that loses text, and this file's
   contract is that text is appended and not removed.
3. **Any other hunk with a non-empty base side is left conflicted**, with markers,
   and the exit is non-zero. Two branches that rewrote the same existing text
   genuinely disagree, and unioning there is how a stale figure survives
   beside its own correction. That is the defect `merge=union` produced on
   `docs/WORKLIST.md` and it is not being reintroduced.

**Nothing in step 2 can drop text**, which is what makes it safe; what it
could do is keep two things that should have been one, and that failure is
caught rather than assumed: `ci/check-doc-claims.py` refuses a duplicated
`## N.` section number and `ci/check-figures.py` refuses a figure whose
print-count in the document drifts from its ledger entry. Both fired for real
on 2026-08-17 — the ledger caught `ow-r16-12345-floor-n15` going from 7
prints to 8 when `main` and `worldgen-aggregate` each added a section quoting
the same floor — so the union rule ships with a live falsifier and not with an
argument.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile

OURS_M = b"<<<<<<< ours"
BASE_M = b"||||||| base"
SPLIT_M = b"======="
THEIRS_M = b">>>>>>> theirs"


def read_bytes(path: str) -> bytes:
    with open(path, "rb") as f:
        return f.read()


def write_bytes(path: str, data: bytes) -> None:
    with open(path, "wb") as f:
        f.write(data)


def whole_file_markers(base: bytes, ours: bytes, theirs: bytes) -> bytes:
    """The last-resort conflict: each side entire, between markers.

    Only reached now if `git merge-file` itself could not be run. Kept because
    a driver that cannot merge must still produce something git recognises as
    a conflict rather than exiting non-zero over an unwritten file.
    """
    return (
        OURS_M + b"\n" + ours +
        (b"" if ours.endswith(b"\n") or not ours else b"\n") +
        BASE_M + b"\n" + base +
        (b"" if base.endswith(b"\n") or not base else b"\n") +
        SPLIT_M + b"\n" + theirs +
        (b"" if theirs.endswith(b"\n") or not theirs else b"\n") +
        THEIRS_M + b"\n"
    )


def git_three_way(base: bytes, ours: bytes, theirs: bytes):
    """git's own line-level 3-way merge. Returns (text, rc) or None if git
    could not be run at all — never raises into the driver's exit path."""
    d = tempfile.mkdtemp(prefix="merge-appendonly-")
    try:
        paths = {}
        for name, data in (("base", base), ("ours", ours), ("theirs", theirs)):
            paths[name] = os.path.join(d, name)
            write_bytes(paths[name], data)
        try:
            r = subprocess.run(
                ["git", "merge-file", "--diff3",
                 "-L", "ours", "-L", "base", "-L", "theirs",
                 paths["ours"], paths["base"], paths["theirs"]],
                capture_output=True,
            )
        except OSError:
            return None
        # git merge-file returns the conflict count, or a value >= 128 for an
        # error it could not merge through. Only the latter is unusable.
        if r.returncode >= 128 or r.returncode < 0:
            return None
        return read_bytes(paths["ours"]), r.returncode
    finally:
        for f in ("base", "ours", "theirs"):
            try:
                os.unlink(os.path.join(d, f))
            except OSError:
                pass
        try:
            os.rmdir(d)
        except OSError:
            pass


def nonblank(part: list[bytes]) -> bool:
    """A side that is empty or only whitespace is a DELETION of the base
    region, and a deletion is the one edit that loses text. Never resolve one
    automatically in a file whose whole contract is that text is appended and
    not removed — leave it for a human, however unambiguous it looks."""
    return b"".join(part).strip() != b""


def union_appended_hunks(text: bytes) -> tuple[bytes, int, int]:
    """Resolve only the hunks where the BASE side is empty.

    An empty base means neither branch changed anything that was there: both
    added their own new text at the same anchor, which is this file's whole
    shape. Everything else is left exactly as git wrote it.

    Returns (text, unioned, refused).
    """
    lines = text.split(b"\n")
    out: list[bytes] = []
    i = 0
    unioned = refused = 0
    n = len(lines)
    while i < n:
        if not lines[i].startswith(OURS_M):
            out.append(lines[i])
            i += 1
            continue
        # A hunk git wrote: ours / base / theirs, in that order. If any part
        # is missing the file is not what we think it is — copy it out
        # verbatim and let the human see it.
        o: list[bytes] = []
        b: list[bytes] = []
        t: list[bytes] = []
        j = i + 1
        while j < n and not lines[j].startswith(BASE_M):
            o.append(lines[j]); j += 1
        if j >= n:
            out.extend(lines[i:]); break
        j += 1
        while j < n and lines[j] != SPLIT_M:
            b.append(lines[j]); j += 1
        if j >= n:
            out.extend(lines[i:]); break
        j += 1
        while j < n and not lines[j].startswith(THEIRS_M):
            t.append(lines[j]); j += 1
        if j >= n:
            out.extend(lines[i:]); break
        if b"".join(b).strip() == b"":
            out.extend(o)
            out.extend(t)
            unioned += 1
        elif nonblank(t) and o and len(o) > len(b) and o[-len(b):] == b:
            # ADJACENCY, NOT DISAGREEMENT. One side left the base region
            # byte-identical and added text immediately above it; the other
            # edited the region. git lumps the two into one hunk because they
            # touch, not because they conflict. Keep the addition, take the
            # edit. Measured 2026-08-17: this is the whole of what was left of
            # clamped-floor-probe's conflict — main appended a section above
            # a `## 36.` heading the branch had reworded.
            out.extend(o[:-len(b)])
            out.extend(t)
            unioned += 1
        elif nonblank(o) and t and len(t) > len(b) and t[-len(b):] == b:
            out.extend(t[:-len(b)])
            out.extend(o)
            unioned += 1
        elif nonblank(t) and o and len(o) > len(b) and o[:len(b)] == b:
            out.extend(t)
            out.extend(o[len(b):])
            unioned += 1
        elif nonblank(o) and t and len(t) > len(b) and t[:len(b)] == b:
            out.extend(o)
            out.extend(t[len(b):])
            unioned += 1
        else:
            refused += 1
            out.append(lines[i]); out.extend(o)
            out.append(BASE_M); out.extend(b)
            out.append(SPLIT_M); out.extend(t)
            out.append(lines[j])
        i = j + 1
    return b"\n".join(out), unioned, refused


def merge(base: bytes, ours: bytes, theirs: bytes) -> tuple[bytes, bool, str]:
    """Returns (result, clean, note)."""
    if ours.startswith(base) and theirs.startswith(base):
        ours_tail = ours[len(base):]
        theirs_tail = theirs[len(base):]
        return base + ours_tail + theirs_tail, True, "both sides pure appends"

    tw = git_three_way(base, ours, theirs)
    if tw is None:
        return (whole_file_markers(base, ours, theirs), False,
                "git merge-file could not be run — whole-file conflict")
    text, rc = tw
    if rc == 0:
        return text, True, "line-level 3-way, no overlapping edits"
    text, unioned, refused = union_appended_hunks(text)
    if refused == 0:
        return (text, True,
                f"line-level 3-way, {unioned} append/append hunk(s) unioned")
    return (text, False,
            f"{refused} hunk(s) rewrite text both sides changed"
            + (f", {unioned} append/append hunk(s) already unioned" if unioned else ""))


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print("merge-appendonly: usage: BASE OURS THEIRS", file=sys.stderr)
        return 2
    base_path, ours_path, theirs_path = argv[1], argv[2], argv[3]
    base = read_bytes(base_path)
    ours = read_bytes(ours_path)
    theirs = read_bytes(theirs_path)

    result, clean, note = merge(base, ours, theirs)
    write_bytes(ours_path, result)

    if clean:
        print(f"merge-appendonly: merged — {note}", file=sys.stderr)
        return 0

    print(
        f"merge-appendonly: refusing — {note}. What is left conflicted is "
        "text both branches changed, and only that; everything either side "
        "merely appended has already been merged.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
