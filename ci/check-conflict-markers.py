#!/usr/bin/env python3
"""Refuse any tracked file that still carries a git conflict marker.

# WHY THIS EXISTS

2026-08-16: a branch reached the gate with `docs/DETERMINISM.md` concatenated
five times — 32,577 lines against main's 6,762 — carrying stray `|||||||
base` diff3 markers from a botched rebase resolution. Nothing in T0 refused
it. `ci/merge-appendonly.py` behaved correctly: on a real conflict it writes
markers and returns non-zero, and `ci/merge-appendonly-falsify.sh` proves
that. The failure was downstream — an agent committed the marked-up file
anyway — and T0 only noticed by accident, because `ci/check-figures.py`
happened to find every figure appearing exactly 5x the ledger's count. Had
the duplicated file carried no registered figures, it would have merged.
That is catching a corruption through a symptom, not through a control. This
script is the control: it reads every marker git can leave behind
(`<<<<<<<`, `|||||||`, `=======`, `>>>>>>>`) as a property of the tracked
tree itself, independent of what any file happens to be measuring.

# WHY THIS WALKS THE FILESYSTEM RATHER THAN ASKING GIT

`ci/setup-merge-drivers.sh`'s header (read it before changing this) records
the fact that matters here: **T0 usually runs where `.git` does not exist.**
`ci/t0-remote.sh` rsyncs with `--exclude '.git'` on purpose, and that rsynced
tree is what `ci/handoff.sh` gates and what every agent's own inner loop
gates too. A check written against `git ls-files` would pass its own author's
worktree and then run against nothing in every gate this project actually
runs — which is exactly the defect class this file exists to close, aimed at
itself. So "tracked" is approximated the same way the shell-parse sweep at
the top of `ci/t0.sh` already approximates it: walk the tree, exclude the
directories `.gitignore` already excludes (build output, the vanilla
extract, generated tables, log/state dirs), and treat what's left as the
corpus. This is the same operational definition T0's very first check uses,
not a new one.

# THE FOUR MARKER FORMS AND WHY THEY ARE ANCHORED THE WAY THEY ARE

Git writes, at the *start* of a line, with nothing before them:

    <<<<<<< <label>       (ours / HEAD side)
    ||||||| <label>       (diff3 common-ancestor side — only with
                            merge.conflictStyle=diff3, which is what a
                            "botched rebase resolution" almost always means)
    =======                (no label, ever)
    >>>>>>> <label>       (theirs side)

A guard that only greps `<<<<<<<` and `>>>>>>>` would have missed exactly the
variant that appeared here — diff3's `|||||||` — which is why all four are
required, not three plus an assumption. Each is matched only at true column
0 (`re.MULTILINE` `^`, which after a `\n` or at file start is never inside a
word or preceded by other prose) and only when the marker run is followed by
a space/tab (the label), `\r`/end-of-line (git's own no-label case, plus
CRLF tolerance), or true end of line — never by an 8th copy of the marker
character, which is what stops a divider line of nine `<` from matching a
false 7. This is also what a line of prose containing seven angle brackets
mid-sentence cannot satisfy: prose is never the first byte of its own line
*and* immediately followed by only whitespace or EOL for exactly seven
repeats — anything else on that line breaks the match at the first
non-matching byte.

# WHY NO PATH ALLOWLIST WAS NEEDED

`ci/merge-appendonly.py` contains the four markers as byte-string literals
(indented Python source: `        b"<<<<<<< ours\n"` — column 0 is a space,
not `<`), `ci/merge-appendonly-falsify.sh` greps for them inside a quoted
shell string (`check "case 1: ..." "$(... grep -qE '^(<<<<<<<|...'`` — column
0 is `check`, not the marker), and this file's own docstring above quotes
them inside backticked prose (`` `<<<<<<<`/`|||||||`/... `` — column 0 is a
backtick). None of them puts a live marker at the true start of a physical
line, because none of them is an actual unresolved conflict — they are all
talking *about* markers, and talking about a thing never requires writing it
where only the thing itself is allowed to sit. Verified empirically, not
just argued: `--selftest` and a real run against this tree at HEAD are both
clean with zero exclusions. If some future file's legitimate marker text
ever does land at column 0 (a heredoc, a raw fixture dropped in unindented),
name it in `LEGITIMATE_MARKER_FILES` below with a one-line reason — that is
the honest version of an exception, because it is reviewed and visible, and
it is deliberately not a glob over `ci/` or anywhere else: a glob is where
the next real corruption would hide.

Usage:
    ci/check-conflict-markers.py               adjudicate the real tree, exit 0/1
    ci/check-conflict-markers.py --root DIR     adjudicate DIR instead (for the
                                                 falsify harness)
    ci/check-conflict-markers.py --selftest     prove the rule fires on fixtures,
                                                 exit 0/1

# THE SECOND RULE: A WHOLE FILE THAT IS EXACTLY N COPIES OF ITSELF

The brief for this script asks the question directly: a whole-file
duplication check would have caught the 2026-08-16 corruption even with no
markers present — a botched resolution that happened to keep every side and
delete the markers by hand would have produced exactly this shape, and
nothing above would see it. Added because it turned out to be both cheap and
exact rather than heuristic: for k in 2..8, does the file's length divide
evenly by k, and are all k chunks byte-identical? That is not a similarity
score with a threshold to tune — it is a yes/no equality test — so it carries
no false-positive risk that scales with a knob somebody has to guess right.
Measured against every tracked file in this repo at HEAD (921 files, 38 MB):
**zero** false positives, in well under a second. `WHOLE_FILE_DUP_MIN_BYTES`
exists only to keep a near-empty or placeholder file (this repo tracks a
0-byte `cave_air`) from trivially satisfying "0 bytes split into k pieces of
0 bytes are equal."
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Mirrors `.gitignore` (and the exclude set the shell-parse sweep at the top
# of `ci/t0.sh` already uses for `.git`/`target`/`oracle-work`), because this
# script has no git to ask and this is the same corpus by another name. Each
# entry is a directory *relative to the scan root*; a file anywhere under one
# of these is skipped without being read.
EXCLUDE_DIRS = {
    ".git",
    "target",
    "oracle-work",
    "data/vanilla",
    "vanilla-src",
    "swarm/logs",
    "swarm/state",
    "ci/logs",
    "__pycache__",
    ".claude",
    ".plugin-hook-falsify-backup",
    "logs",
}

# See "WHY NO PATH ALLOWLIST WAS NEEDED" above. Empty because the anchored
# regex already discriminates every known case correctly — this exists so a
# genuine future exception has somewhere honest to go, named and reasoned,
# rather than a silent skip of a whole directory.
LEGITIMATE_MARKER_FILES: dict[str, str] = {
    # "path/from/repo/root.ext": "why this file's marker text is not a live conflict",
}

MARKER_RE = re.compile(
    rb"^(?:"
    rb"<{7}(?:[ \t].*)?\r?$"  # <<<<<<< ours   /  <<<<<<< HEAD
    rb"|\|{7}(?:[ \t].*)?\r?$"  # ||||||| base   (diff3 only — the one that appeared)
    rb"|={7}\r?$"  # =======        (git never gives this one a label)
    rb"|>{7}(?:[ \t].*)?\r?$"  # >>>>>>> theirs
    rb")",
    re.MULTILINE,
)

# Below this size the "file == k equal chunks" test is trivially true (an
# empty file, or a couple of bytes) and would just be noise — see "THE SECOND
# RULE" in the module docstring.
WHOLE_FILE_DUP_MIN_BYTES = 256
WHOLE_FILE_DUP_MAX_PERIOD = 8


def whole_file_duplicate_period(data: bytes) -> int | None:
    """If `data` is exactly k >= 2 back-to-back identical copies of a shorter
    block, for some k in 2..WHOLE_FILE_DUP_MAX_PERIOD, return the smallest
    such k. Otherwise None. Exact byte equality only — no threshold, no
    fuzzy match."""
    n = len(data)
    if n < WHOLE_FILE_DUP_MIN_BYTES:
        return None
    for k in range(2, WHOLE_FILE_DUP_MAX_PERIOD + 1):
        if n % k != 0:
            continue
        unit = n // k
        chunk0 = data[:unit]
        if all(data[i * unit:(i + 1) * unit] == chunk0 for i in range(1, k)):
            return k
    return None


def iter_files(root: Path):
    """Yield every regular file under `root` not pruned by EXCLUDE_DIRS,
    as a (absolute_path, root_relative_posix_str) pair."""
    for p in sorted(root.rglob("*")):
        if not p.is_file():
            continue
        rel = p.relative_to(root)
        rel_posix = rel.as_posix()
        parts = rel.parts
        pruned = False
        for i in range(1, len(parts) + 1):
            if "/".join(parts[:i]) in EXCLUDE_DIRS:
                pruned = True
                break
        if pruned:
            continue
        yield p, rel_posix


def check(root: Path) -> list[str]:
    """Returns a list of red lines — one per marker occurrence, plus one per
    whole-file exact duplication — each naming the file (and, for a marker,
    the line). Empty means clean."""
    reds: list[str] = []
    for path, rel in iter_files(root):
        if rel in LEGITIMATE_MARKER_FILES:
            continue
        try:
            data = path.read_bytes()
        except OSError as e:
            reds.append(f"UNREADABLE {rel}: {e}")
            continue
        for m in MARKER_RE.finditer(data):
            line_no = data.count(b"\n", 0, m.start()) + 1
            snippet = data[m.start():m.start() + 40].split(b"\n", 1)[0]
            reds.append(
                f"CONFLICT-MARKER {rel}:{line_no}: "
                f"{snippet.decode('utf-8', 'replace')!r}"
            )
        k = whole_file_duplicate_period(data)
        if k is not None:
            unit = len(data) // k
            reds.append(
                f"WHOLE-FILE-DUPLICATE {rel}: the whole file is {k} back-to-back "
                f"exact copies of a {unit}-byte block ({len(data)} bytes total) "
                "— no conflict marker needed to catch this"
            )
    return reds


# ---------------------------------------------------------------------------
# self-test — fixtures only, no real tree


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

    # --- a clean tree passes -------------------------------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        (d / "docs").mkdir()
        (d / "docs" / "DETERMINISM.md").write_text("## 1. Foo\nbody\n\n## 2. Bar\nbody\n")
        reds = check(d)
        expect("a clean tree has no reds", reds == [])

    # --- each of the four marker forms, on its own, is caught ----------
    cases = {
        "ours-side": b"before\n<<<<<<< HEAD\nour text\n",
        "diff3-base": b"before\n||||||| base\ncommon ancestor text\n",
        "separator": b"before\n=======\nafter\n",
        "theirs-side": b"before\n>>>>>>> feature-branch\nafter\n",
    }
    for name, body in cases.items():
        with tempfile.TemporaryDirectory() as td:
            d = Path(td)
            (d / "f.md").write_bytes(body)
            reds = check(d)
            expect(f"{name} marker is caught", any("CONFLICT-MARKER" in r for r in reds))

    # --- a real four-marker conflict names every line -------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        body = (
            b"<<<<<<< ours\n"
            b"our line\n"
            b"||||||| base\n"
            b"base line\n"
            b"=======\n"
            b"their line\n"
            b">>>>>>> theirs\n"
        )
        (d / "f.md").write_bytes(body)
        reds = check(d)
        expect("all four markers in one file are all caught", len(reds) == 4)
        expect("line numbers are 1/3/5/7", all(
            f"f.md:{n}:" in "".join(reds) for n in (1, 3, 5, 7)
        ))

    # --- prose that merely mentions markers is not flagged --------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        body = (
            b"The driver writes `<<<<<<<`/`|||||||`/`=======`/`>>>>>>>` markers.\n"
            b"        b\"<<<<<<< ours\\n\" + ours +\n"
            b"check \"markers\" \"$(grep -qE '^<<<<<<<' file)\"\n"
        )
        (d / "prose.py").write_bytes(body)
        reds = check(d)
        expect("marker text not at column 0 is not a false positive", reds == [])

    # --- a longer run than 7 does not falsely match as a marker ---------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        (d / "divider.md").write_bytes(b"<<<<<<<<<< a wide divider, not a marker\n")
        reds = check(d)
        expect("an 8+ run of the marker char is not flagged", reds == [])

    # --- whole-file exact duplication, no markers involved at all --------
    # The shape this exists for: docs/DETERMINISM.md concatenated five
    # times, markers stripped by hand, so nothing above this rule sees it.
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        unit = b"## 1. Section\n" + b"x" * 300 + b"\n"
        (d / "f5.md").write_bytes(unit * 5)
        reds = check(d)
        dup = [r for r in reds if r.startswith("WHOLE-FILE-DUPLICATE")]
        expect("a file that is 5 exact copies of itself is caught", len(dup) == 1)
        expect("the duplicate names k=5", dup and " 5 back-to-back" in dup[0])
        expect("a pure duplication carries no marker red", not any(
            r.startswith("CONFLICT-MARKER") for r in reds
        ))

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        unit = b"body text that repeats\n" * 20
        (d / "f2.md").write_bytes(unit * 2)
        reds = check(d)
        expect("k=2 whole-file duplication is also caught", any(
            r.startswith("WHOLE-FILE-DUPLICATE") for r in reds
        ))

    # --- near-duplicate (one byte different) must NOT trigger ------------
    # This is the point of an exact test rather than a similarity score: a
    # file that legitimately differs between its "halves" must pass, with no
    # threshold to tune.
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        half = b"## 1. Section\n" + b"x" * 300 + b"\n"
        other_half = half[:-2] + b"y\n"  # one byte different
        (d / "near.md").write_bytes(half + other_half)
        reds = check(d)
        expect(
            "an almost-but-not-exactly duplicated file is not flagged",
            not any(r.startswith("WHOLE-FILE-DUPLICATE") for r in reds),
        )

    # --- small/trivial files do not trip the trivial "0 == 0" case -------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        (d / "empty.txt").write_bytes(b"")
        (d / "tiny.txt").write_bytes(b"aa")  # 2 bytes, "aa" = "a"*2, k=2 would match
        reds = check(d)
        expect(
            "an empty file is not flagged as a whole-file duplicate",
            not any("empty.txt" in r for r in reds),
        )
        expect(
            "a file below WHOLE_FILE_DUP_MIN_BYTES is not flagged even if it "
            "trivially repeats",
            not any("tiny.txt" in r for r in reds),
        )

    # --- EXCLUDE_DIRS actually prunes ------------------------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        (d / "target").mkdir()
        (d / "target" / "bad.txt").write_bytes(b"<<<<<<< HEAD\n")
        (d / "oracle-work").mkdir()
        (d / "oracle-work" / "bad.txt").write_bytes(b"<<<<<<< HEAD\n")
        reds = check(d)
        expect("excluded directories are not scanned", reds == [])

    # --- LEGITIMATE_MARKER_FILES, if ever populated, is honoured --------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        (d / "allowed.txt").write_bytes(b"<<<<<<< HEAD\n")
        saved = dict(LEGITIMATE_MARKER_FILES)
        try:
            LEGITIMATE_MARKER_FILES["allowed.txt"] = "test fixture for the allowlist path"
            reds = check(d)
            expect("an explicitly allowlisted path is skipped", reds == [])
        finally:
            LEGITIMATE_MARKER_FILES.clear()
            LEGITIMATE_MARKER_FILES.update(saved)

    print("SELFTEST GREEN" if ok else "SELFTEST RED")
    return 0 if ok else 1


def main() -> int:
    argv = sys.argv[1:]
    if "--selftest" in argv:
        return _selftest()

    root = ROOT
    if "--root" in argv:
        root = Path(argv[argv.index("--root") + 1]).resolve()

    reds = check(root)
    if reds:
        for r in reds:
            print(f"check-conflict-markers: {r}", file=sys.stderr)
        n_marker = sum(1 for r in reds if r.startswith("CONFLICT-MARKER"))
        n_dup = sum(1 for r in reds if r.startswith("WHOLE-FILE-DUPLICATE"))
        n_other = len(reds) - n_marker - n_dup
        print(
            f"check-conflict-markers: {n_marker} conflict marker(s), "
            f"{n_dup} whole-file duplicate(s), {n_other} other issue(s) "
            "in the tracked tree",
            file=sys.stderr,
        )
        return 1
    print("check-conflict-markers: 0 conflict markers, 0 whole-file duplicates in the tracked tree")
    return 0


if __name__ == "__main__":
    sys.exit(main())
