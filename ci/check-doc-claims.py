#!/usr/bin/env python3
"""Adjudicate docs/claims/*.toml against the `## N.` headings they register.

# WHY THIS EXISTS

`docs/DETERMINISM.md` is append-only with hand-numbered `## N.` sections, and
`docs/WORKLIST.md`'s COLD START blocks used to be inserted at the same anchor
25 times over. Two branches that each append cannot both rebase cleanly: they
collide textually at the same line, and they usually also pick the **same
section number** — a silent collision that costs a merge chain an
orchestrator turn to resolve by hand, every time. Merge chains 32, 33 and 34
stranded nine branch-slots between them on exactly this and merged two
branches out of eleven attempts.

The fix is not a smarter merge. It is removing the shared line: a claimed
number is now a **file**, `docs/claims/<doc>-<N>-<slug>.toml`, so two branches
claiming a number at once produce two files rather than one conflicted line.
This script is the part that makes that structural rather than decorative —
a registry that can silently drift from the headings it registers is not a
registry, so it reddens on:

* two claim files naming the same `(doc, number)` — a duplicate, printing
  every claimant so the fix is one rename, not an investigation;
* the same `## N.` heading appearing more than once in one document — added
  2026-08-12. The three checks below it read `find_headings`'s output through
  a `set`, so two physical occurrences of `## 22.` and one of `## 26.`
  collapse to the same two-element set as one occurrence of each: a checker
  "reading the documents" was still blind to a renumber that left the old
  heading in place, because *presence* and *count* are different questions
  and only presence was ever asked. `docs/PARITY-ROADMAP.md` carried exactly
  that duplicate on `main` with a claim file on each side of it — see
  `ci-doc-heading-guard`'s cold start;
* a `## N.` heading in a registered doc with no claim file behind it;
* a claim file with no `## N.` heading behind it;
* a claim file whose own filename disagrees with the fields inside it — the
  copy-paste-without-rename mistake this project has hit before
  (`docs/AGENT-BRIEF.md`'s WORKLIST splice-on-heading-string note is the same
  class: a name that drifts from what it names);
* **a section CITATION pointing at a heading that does not exist** — added
  2026-08-17, and it is the half the four rules above are structurally blind
  to. Every one of them adjudicates the registry against the headings; none
  looks at the 310 places in this tree that *cite* a section by number, in
  prose, in `ci/checks/*.toml` and in Rust doc comments. Proven rather than
  argued: renumber a section and move its claim file and claim filename with it
  — the tidy, correct thing an agent does — and this script on `main` reports
  "79 claims registered, 0 collisions, 0 drift" and exits 0 while 19 citations
  of the old number point at nothing. It is a ratchet, not an absolute, because
  a forward citation of a section still on an unmerged branch is legitimate
  here and there are eight of them; see MAX_DANGLING_SECTION_REFS.

# WHAT THIS DELIBERATELY DOES NOT DO

It does not renumber anything, adjudicate section *content*, or care what a
title says beyond using it for the filename cross-check. And it does not
fire on `docs/WORKLIST.md`: that file is self-evolving (the three `## NOW`
blocks), not append-only, and register-by-number is the wrong model for a
paragraph that gets edited in place — see `docs/AGENT-BRIEF.md` and the
`## NOW` note in `docs/WORKLIST.md` itself.

# A KNOWN, INTENDED HOLE — DO NOT "FIX" A MISSING NUMBER ON SIGHT

`docs/PARITY-ROADMAP.md` skips `## 18.` on every branch that has not landed
it: two branches each independently add a *different* `## 18.`
(`0878cc4d`, `4b532a71`), and a third (`gameplay-gap-rederive`) took `19` to
avoid a three-way collision rather than fight for `18`. A branch with no
`## 18.` heading and no claim for it is not a defect — HEADING-NO-CLAIM and
CLAIM-NO-HEADING both stay silent on a number neither side has, by
construction, so this script does not need to know the hole exists. The
moment both of those branches' claim files land together, two files will
name `(PARITY-ROADMAP, "18")` and DUPLICATE-CLAIM will fire correctly. That
is the point: the hole is invisible on any one branch and loud the instant
it would actually collide.

# THE DEFECT CLASS THIS MUST NOT JOIN

"A check that could not fail" has been found in this project many times over
— a ledger-style checker is unusually easy to write in that shape: an empty
`docs/claims/` directory passes, a doc with no headings passes, a filename
check that never disagrees with its own fields is decoration. `--selftest`
below poisons every rule above on fixtures — no real docs, no real claims
directory — and asserts each one is actually caught.

Usage:
    ci/check-doc-claims.py             adjudicate the real tree, exit 0/1/2
    ci/check-doc-claims.py --selftest  prove the rules can fire, exit 0/1
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

if sys.version_info < (3, 11):
    print("check-doc-claims: needs python 3.11 for tomllib", file=sys.stderr)
    sys.exit(2)
import tomllib

ROOT = Path(__file__).resolve().parent.parent

# The append-only docs this registry covers. `docs/WORKLIST.md` is
# deliberately absent — see the module docstring.
DOCS = {
    "DETERMINISM": "DETERMINISM.md",
    "PARITY-ROADMAP": "PARITY-ROADMAP.md",
}

# `## 4.` needs a trailing period; `## 4.1` (a sub-section) does not, because
# the decimal point already terminates the number. Both forms are live in
# `docs/DETERMINISM.md` today (`## 4.1` .. `## 4.5`, `## 17.1`).
HEADING_RE = re.compile(r"^## (\d+(?:\.\d+)?)\.?\s+\S")

SLUG_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")

REQUIRED_FIELDS = ("doc", "number", "slug", "title", "branch")

# `status` is optional and defaults to "written", so every claim file that
# existed before 2026-08-18 stays valid without being touched.
#
#   written   the `## N.` heading exists in the doc. The original contract.
#   reserved  the number is TAKEN but the section is NOT WRITTEN YET.
#
# Until `reserved` existed there was no honest way to hold a number in
# advance: a claim file with no heading is CLAIM-NO-HEADING, a red. So the
# registry refused the honest act — reserve, then write — and permitted the
# dishonest one, which is to invent a number, write the section under it, and
# discover at merge that a lane you cannot see invented the same one. That is
# the §33, §54 and "is 54 taken?" incidents of 2026-08-18, all three.
#
# `reserved` is NOT a hole to park a claim in. It is exempt from
# CLAIM-NO-HEADING and from nothing else: it is fully counted by
# DUPLICATE-CLAIM, and RESERVED-BUT-WRITTEN reddens the moment the heading
# actually appears, so a reservation that has quietly become a real section
# cannot sit there mislabelled. The state is falsifiable in both directions.
#
# `ci/claim-number.py` writes these; see its docstring for the allocator.
CLAIM_STATUSES = ("written", "reserved")

# --- the renumber orphan, and why it is a ratchet --------------------------
#
# 2026-08-17. The claim registry above stops two branches picking one number.
# What it never looked at is the 310 places in this tree that *cite* a section
# by number — DETERMINISM section 15, PARITY-ROADMAP section 20.9 — in
# prose, in `ci/checks/*.toml`, and in Rust doc comments. A renumber moves the
# heading and leaves every citation of it pointing at nothing, which is how
# 2026-08-17 produced a §44->§51 and a §27->§46 renumber that a rebase
# discovered rather than a check.
#
# ONLY QUALIFIED REFERENCES ARE SCANNED — a `§N` that names its document on the
# same line. A bare `§14` inherits its document from surrounding prose and
# resolving it would need to guess, and a guessed target is worse than none.
# 310 of them are qualified, which is plenty of coverage to hold.
#
# ALL HEADING LEVELS COUNT, not just `## N.`. Sub-sections are written `### 15.4`
# and `#### 20.10`, they are cited exactly as often as top-level ones, and
# checking references against `##` alone reported 70 dangling where 8 are:
# 62 false positives, which would have made this rule the thing agents route
# around. Measured before shipping rather than after.
#
# THE CEILING IS 8 AND NOT 0, and that is the honest number rather than a
# concession. All eight of today's dangling references are FORWARD references to
# sections on branches that have not merged — PARITY-ROADMAP section 32 (x3,
# and `docs/cold-start/server-protocol-ids-path.md` says in as many words that
# the branch is unmerged) and DETERMINISM sections 33, 33.2 and 33.4 (x5, cited
# from `crates/rustmc-server/src/growing.rs` and `tiles.rs`). DETERMINISM is
# missing exactly 33 and 34 and PARITY-ROADMAP exactly 32, which corroborates
# it. A forward reference is legitimate here and refusing one would be refusing
# the repo's own working pattern; what may not happen is the count RISING from
# a renumber that ORPHANS a citation.
#
# RAISED 8 -> 9 ON 2026-08-18, AND THE DIRECTION MATTERS. This is not a
# renumber orphaning a citation, which is the case the ceiling exists to
# refuse. `docs/cold-start/ci-figures-ledger.md` cited `docs/DETERMINISM.md`
# §35 four times for content that is **not in §35**: `worldgen-aggregate`
# claimed 35, `worldgen-deliver-once` landed 35 first with unrelated
# chunk-canvas content, and `worldgen-aggregate` renumbered itself to §33 on
# 2026-08-16 (695a66d5) to get out of the way. So those four references
# RESOLVED — to the wrong section — and a citation that resolves to the wrong
# place is invisible to this check forever, which is strictly worse than one
# that dangles. Correcting them to §33 makes the file honest and the error
# visible, at the price of one more dangling ref until `worldgen-aggregate`
# merges. Trading an invisible wrong reference for a visible forward one is the
# direction this script exists to encourage.
#
# The other eight are unchanged and the ceiling drops back to 0 when
# PARITY-ROADMAP §32 and DETERMINISM §33 land.
#
# The bound is a constant in this script rather than a field in a data file, so
# widening it is a reviewed change and not a data edit — AGENT-BRIEF's rule for
# every other ratchet here.
MAX_DANGLING_SECTION_REFS = 9

# Any `## N.` / `### N.M` / `#### N.M` heading, at any of the three levels.
ANY_NUMBERED_HEADING_RE = re.compile(r"^#{2,4} (\d+(?:\.\d+)?)\.?(?:\s|$)")

# `docs/DETERMINISM.md` §15 — the document named, then the section, close by.
# The 24-character gap absorbs a closing backtick, a comma or a short word
# ("`docs/DETERMINISM.md` §12 and §15" resolves only the first, deliberately:
# the second is unqualified).
SECTION_REF_RE = re.compile(
    r"docs/(DETERMINISM|PARITY-ROADMAP)\.md`?[^\n]{0,24}?§\s?(\d+(?:\.\d+)?)"
)

# Where a citation can live. Everything that is tracked prose or tracked source;
# a missing directory is skipped, which is what lets the self-test run on a
# three-file fixture.
REF_SCAN = (
    ("", ("README.md",)),
    ("docs", ("*.md",)),
    ("ci", ("*.py", "*.sh", "*.toml")),
    ("tools", ("*.rs",)),
    ("crates", ("*.rs",)),
)


def canon_number(value) -> str:
    """`4` and `4.1` both round-trip through TOML as bare numbers (int/float).

    Format rather than `str()` a float directly: Python's float repr is
    already the shortest round-tripping form for the one-decimal-place
    numbers this project uses (`4.1`, `17.1`), but going through `%g`
    guards against a value like `4.10` ever printing a trailing zero that
    would silently split one section into two keys.
    """
    if isinstance(value, float):
        return "%g" % value
    return str(value)


def sort_key(number: str):
    return tuple(float(part) for part in number.split("."))


def find_headings(doc_path: Path) -> list[tuple[str, int]]:
    """Every `## N.` heading as (number, 1-based line number), in file order,
    duplicates included. A `set` here is exactly the hole DUPLICATE-HEADING
    exists to close: this returned `set[str]` until 2026-08-12, which made the
    one collision the tool exists to prevent the one collision it could not
    see. A doc holding two `## 22.` headings deduplicated to a single `"22"`,
    matched the single claim file for 22, and the checker printed
    `0 collisions`. Found on `main` at 84f789db: a renumber inserted `## 26.`
    and left the old `## 22.` line above it, so `docs/PARITY-ROADMAP.md` had
    two sections numbered 22 and a reader following a `§22` cross-reference
    could land on either. The registry was consistent; the doc was not, and
    only the doc is read. Callers that only need presence can still build a
    set from this, but the count and the line numbers are the point of
    returning a list."""
    out: list[tuple[str, int]] = []
    for i, line in enumerate(doc_path.read_text(encoding="utf-8").splitlines(), start=1):
        m = HEADING_RE.match(line)
        if m:
            out.append((m.group(1), i))
    return out


def load_claims(claims_dir: Path) -> tuple[list[dict], list[str]]:
    """Returns (claims, reds). A claim that fails to parse or validate is
    dropped from `claims` and recorded as a red rather than silently
    excluded — a malformed claim must not read as an absent one."""
    claims: list[dict] = []
    reds: list[str] = []
    if not claims_dir.is_dir():
        return claims, reds
    for p in sorted(claims_dir.glob("*.toml")):
        try:
            t = tomllib.loads(p.read_text(encoding="utf-8"))
        except tomllib.TOMLDecodeError as e:
            reds.append(f"MALFORMED {p.name}: does not parse as TOML: {e}")
            continue
        missing = [k for k in REQUIRED_FIELDS if k not in t]
        if missing:
            reds.append(f"MALFORMED {p.name}: missing field(s) {missing}")
            continue
        doc = t["doc"]
        if doc not in DOCS:
            reds.append(
                f"MALFORMED {p.name}: doc `{doc}` is not a registered "
                f"append-only doc (known: {sorted(DOCS)})"
            )
            continue
        number = canon_number(t["number"])
        slug = t["slug"]
        if not SLUG_RE.match(slug):
            reds.append(
                f"MALFORMED {p.name}: slug `{slug}` is not lowercase "
                "hyphen-separated ([a-z0-9-]+, no leading/trailing/double hyphen)"
            )
            continue

        # Reconstruct the filename the claim's own fields demand, rather than
        # trying to split the actual filename back into (doc, number, slug):
        # a slug may itself contain digits and hyphens (`20260722-per-block...`,
        # `040-coin-flip`), which makes that split ambiguous. Comparing forward
        # — fields to expected name — has exactly one answer.
        expected = f"{doc}-{number}-{slug}.toml"
        if p.name != expected:
            reds.append(
                f"FILENAME-MISMATCH {p.name}: the file's own fields "
                f"(doc={doc}, number={number}, slug={slug}) name it {expected}"
            )
            continue

        status = t.get("status", "written")
        if status not in CLAIM_STATUSES:
            reds.append(
                f"MALFORMED {p.name}: status `{status}` is not one of "
                f"{list(CLAIM_STATUSES)}"
            )
            continue

        claims.append(
            {
                "path": p,
                "doc": doc,
                "number": number,
                "slug": slug,
                "status": status,
                "branch": t["branch"],
            }
        )
    return claims, reds


def check(root: Path) -> tuple[list[str], int]:
    """Returns (reds, n_claims) for the tree rooted at `root`."""
    reds: list[str] = []
    claims, load_reds = load_claims(root / "docs" / "claims")
    reds.extend(load_reds)

    by_key: dict[tuple[str, str], list[Path]] = {}
    for c in claims:
        by_key.setdefault((c["doc"], c["number"]), []).append(c["path"])
    for (doc, number), paths in sorted(by_key.items()):
        if len(paths) > 1:
            names = ", ".join(p.name for p in sorted(paths))
            reds.append(
                f"DUPLICATE-CLAIM {doc} §{number} claimed by {len(paths)} files: {names}"
            )

    for doc_id, filename in DOCS.items():
        doc_path = root / "docs" / filename
        if not doc_path.exists():
            reds.append(f"MISSING-DOC {doc_id}: {doc_path} does not exist")
            continue
        occurrences = find_headings(doc_path)
        by_number: dict[str, list[int]] = {}
        for n, line_no in occurrences:
            by_number.setdefault(n, []).append(line_no)
        for n in sorted(by_number, key=sort_key):
            lines = by_number[n]
            if len(lines) > 1:
                where = ", ".join(f"line {ln}" for ln in lines)
                reds.append(
                    f"DUPLICATE-HEADING {doc_id} §{n} appears {len(lines)} times "
                    f"in {filename} ({where})"
                )

        headings = set(by_number)
        mine = [c for c in claims if c["doc"] == doc_id]
        claimed = {c["number"] for c in mine}
        # A reserved number is claimed for DUPLICATE-CLAIM and for
        # HEADING-NO-CLAIM, and only exempt from CLAIM-NO-HEADING — being
        # unwritten is the whole content of the state.
        reserved = {c["number"] for c in mine if c["status"] == "reserved"}
        for n in sorted(headings - claimed, key=sort_key):
            reds.append(
                f"HEADING-NO-CLAIM {doc_id} §{n} has a heading in {filename} "
                "and no claim file registers it"
            )
        for n in sorted(claimed - headings - reserved, key=sort_key):
            reds.append(
                f"CLAIM-NO-HEADING {doc_id} §{n} has a claim file and no "
                f"`## {n}` heading in {filename}"
            )
        for n in sorted(reserved & headings, key=sort_key):
            reds.append(
                f"RESERVED-BUT-WRITTEN {doc_id} §{n} is registered "
                f'`status = "reserved"` and `## {n}` exists in {filename}. '
                'The section is written; set `status = "written"`'
            )
        n_reserved = len(reserved - headings)
        if n_reserved:
            # Printed on a green run so the unwritten reservations are visible
            # without anyone having to go looking. A reservation nobody can see
            # is the state that was missing, reintroduced.
            held = ", ".join(
                f"§{c['number']} ({c['branch']})"
                for c in sorted(mine, key=lambda c: sort_key(c["number"]))
                if c["status"] == "reserved" and c["number"] not in headings
            )
            print(f"DOC-CLAIMS {doc_id}: {n_reserved} reserved, unwritten — {held}")

    reds.extend(check_section_refs(root))
    return reds, len(claims)


def numbered_headings(doc_path: Path) -> set[str]:
    """Every numbered heading at any of the three levels, as a set of numbers.

    Deliberately NOT `find_headings`: that one is `##`-only because the claim
    registry only governs top-level sections, and a set would defeat its
    duplicate detection. This one is presence-only across all levels, because a
    citation of `§15.4` is satisfied by a `### 15.4`.
    """
    out: set[str] = set()
    for line in doc_path.read_text(encoding="utf-8", errors="replace").splitlines():
        m = ANY_NUMBERED_HEADING_RE.match(line)
        if m:
            out.add(m.group(1))
    return out


def check_section_refs(root: Path, ceiling: int | None = None) -> list[str]:
    """DANGLING-SECTION-REF — a ratchet. See MAX_DANGLING_SECTION_REFS.

    `ceiling` is a parameter only so the self-test can set it to 0 against a
    three-file fixture. Production always uses the module constant, because a
    bound that a caller can pass is a bound a caller can widen.
    """
    if ceiling is None:
        ceiling = MAX_DANGLING_SECTION_REFS
    targets: dict[str, set[str]] = {}
    for doc_id, filename in DOCS.items():
        path = root / "docs" / filename
        if not path.exists():
            return []  # MISSING-DOC already fired; do not pile on
        targets[doc_id] = numbered_headings(path)

    dangling: list[str] = []
    for subdir, patterns in REF_SCAN:
        base = root / subdir if subdir else root
        if not base.exists():
            continue
        for pattern in patterns:
            paths = [base / pattern] if "*" not in pattern else sorted(base.rglob(pattern))
            for path in paths:
                if not path.is_file():
                    continue
                try:
                    text = path.read_text(encoding="utf-8", errors="replace")
                except OSError:
                    continue
                for i, line in enumerate(text.splitlines(), start=1):
                    for doc_id, number in SECTION_REF_RE.findall(line):
                        if number not in targets[doc_id]:
                            rel = path.relative_to(root)
                            dangling.append(f"{rel}:{i} -> {doc_id} §{number}")

    if len(dangling) > ceiling:
        listed = "; ".join(sorted(dangling))
        return [
            f"DANGLING-SECTION-REF {len(dangling)} qualified section references "
            f"point at a heading that does not exist, ceiling is "
            f"{ceiling}. A renumber that orphans a citation is "
            f"caught here rather than by a rebase. {listed}"
        ]
    # Printed on every run, green included, so the ceiling can be LOWERED as the
    # forward references it allows for actually merge. A ratchet nobody can see
    # the slack in only ever ratchets one way.
    print(
        f"DOC-CLAIMS section refs: {len(dangling)} dangling of ceiling "
        f"{ceiling}"
        + (f" ({'; '.join(sorted(dangling))})" if dangling else "")
    )
    return []


# ---------------------------------------------------------------------------
# self-test — fixtures only, no real docs/ and no real docs/claims/


def _write(d: Path, rel: str, body: str) -> None:
    p = d / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(body, encoding="utf-8")


def _claim(doc: str, number: str, slug: str, title: str = "x", branch: str = "test") -> str:
    return f'doc = "{doc}"\nnumber = {number}\nslug = "{slug}"\ntitle = "{title}"\nbranch = "{branch}"\n'


def _base_tree(d: Path) -> None:
    _write(d, "docs/DETERMINISM.md", "## 1. Foo\nbody\n\n## 2. Bar\nbody\n")
    _write(d, "docs/PARITY-ROADMAP.md", "## 1. Baz\nbody\n")
    _write(d, "docs/claims/DETERMINISM-1-foo.toml", _claim("DETERMINISM", "1", "foo"))
    _write(d, "docs/claims/DETERMINISM-2-bar.toml", _claim("DETERMINISM", "2", "bar"))
    _write(d, "docs/claims/PARITY-ROADMAP-1-baz.toml", _claim("PARITY-ROADMAP", "1", "baz"))


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

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        reds, n = check(d)
        expect("clean tree has no reds", reds == [])
        expect("clean tree counts all three claims", n == 3)

    # --- the poisoned case the brief demands: a genuine collision --------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/claims/DETERMINISM-2-bar2.toml", _claim("DETERMINISM", "2", "bar2"))
        reds, n = check(d)
        dup = [r for r in reds if r.startswith("DUPLICATE-CLAIM")]
        expect("duplicate (doc, number) is caught", len(dup) == 1)
        expect(
            "duplicate names both claimants",
            dup and "DETERMINISM-2-bar.toml" in dup[0] and "DETERMINISM-2-bar2.toml" in dup[0],
        )
        expect("duplicate exits red (nonzero rule count)", len(reds) >= 1)

    # --- the same heading twice in one doc (the PARITY-ROADMAP 22/26 shape) -
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(
            d,
            "docs/PARITY-ROADMAP.md",
            "## 1. Baz\nbody\n## 1. Baz again\nbody\n",
        )
        reds, _ = check(d)
        dup = [r for r in reds if r.startswith("DUPLICATE-HEADING")]
        expect("a heading repeated in one doc is caught", len(dup) == 1)
        expect(
            "the duplicate names both line numbers",
            dup and "line 1" in dup[0] and "line 3" in dup[0],
        )
        expect(
            "a repeated-but-claimed number does not also fire HEADING-NO-CLAIM"
            " or CLAIM-NO-HEADING",
            not any(
                r.startswith("HEADING-NO-CLAIM") or r.startswith("CLAIM-NO-HEADING")
                for r in reds
            ),
        )

    # --- heading with nothing registering it ------------------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/DETERMINISM.md", "## 1. Foo\nbody\n\n## 2. Bar\nbody\n\n## 3. Extra\nbody\n")
        reds, _ = check(d)
        expect(
            "heading with no claim is caught",
            any(r.startswith("HEADING-NO-CLAIM DETERMINISM §3") for r in reds),
        )

    # --- two headings with ONE number: the case that passed until 2026-08-12 -
    # Registry-consistent and doc-inconsistent. `find_headings` returned a set,
    # so this deduplicated to a single "2", matched the single claim for 2, and
    # printed `0 collisions`. The positive control below it proves the red is
    # not firing on the shape of the fixture.
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/DETERMINISM.md", "## 1. Foo\nbody\n\n## 2. Bar\nbody\n\n## 2. Bar again\nbody\n")
        reds, _ = check(d)
        dup = [r for r in reds if r.startswith("DUPLICATE-HEADING")]
        expect("two headings with one number is caught", len(dup) == 1)
        expect(
            "duplicate heading names the doc, the number and the count",
            dup and "DETERMINISM §2" in dup[0] and "appears 2 times" in dup[0],
        )
        expect(
            "a duplicated heading raises no spurious HEADING-NO-CLAIM",
            not any(r.startswith("HEADING-NO-CLAIM") for r in reds),
        )

    # --- green control: the same tree with the number corrected -------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/DETERMINISM.md", "## 1. Foo\nbody\n\n## 2. Bar\nbody\n\n## 3. Bar again\nbody\n")
        _write(d, "docs/claims/DETERMINISM-3-bar-again.toml", _claim("DETERMINISM", "3", "bar-again"))
        reds, _ = check(d)
        expect("the de-duplicated tree is green", reds == [])

    # --- claim with nothing behind it --------------------------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/claims/DETERMINISM-9-ghost.toml", _claim("DETERMINISM", "9", "ghost"))
        reds, _ = check(d)
        expect(
            "claim with no heading is caught",
            any(r.startswith("CLAIM-NO-HEADING DETERMINISM §9") for r in reds),
        )

    # --- decimal sub-sections (`4.1`) are not confused with `4` -----------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(
            d,
            "docs/DETERMINISM.md",
            "## 1. Foo\nbody\n\n## 2. Bar\nbody\n\n## 2.1 Sub\nbody\n",
        )
        _write(d, "docs/claims/DETERMINISM-2.1-sub.toml", _claim("DETERMINISM", "2.1", "sub"))
        reds, n = check(d)
        expect("a decimal sub-section claims cleanly", reds == [])
        expect("the decimal claim is counted", n == 4)

    # --- malformed TOML -----------------------------------------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/claims/DETERMINISM-9-broken.toml", "this is not [ valid toml\n")
        reds, _ = check(d)
        expect("malformed toml is caught", any(r.startswith("MALFORMED") for r in reds))

    # --- missing fields -------------------------------------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/claims/DETERMINISM-9-partial.toml", 'doc = "DETERMINISM"\nnumber = 9\n')
        reds, _ = check(d)
        expect(
            "a claim missing required fields is caught",
            any(r.startswith("MALFORMED") and "partial" in r for r in reds),
        )

    # --- filename disagrees with the fields inside it ----------------------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/claims/DETERMINISM-5-decoy.toml", _claim("DETERMINISM", "6", "decoy"))
        reds, _ = check(d)
        expect(
            "filename/field mismatch is caught",
            any(r.startswith("FILENAME-MISMATCH") for r in reds),
        )

    # --- an unregistered doc is refused rather than silently skipped ------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(d, "docs/claims/WORKLIST-9-ghost.toml", _claim("WORKLIST", "9", "ghost"))
        reds, _ = check(d)
        expect(
            "a claim for an unregistered doc (e.g. WORKLIST) is refused",
            any(r.startswith("MALFORMED") and "not a registered" in r for r in reds),
        )

    # --- an empty registry is not a silent pass when headings exist -------
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _write(d, "docs/DETERMINISM.md", "## 1. Foo\nbody\n")
        _write(d, "docs/PARITY-ROADMAP.md", "## 1. Baz\nbody\n")
        reds, n = check(d)
        expect("an empty docs/claims/ directory cannot pass a real doc", len(reds) == 2)
        expect("an empty registry counts zero claims", n == 0)

    # --- DANGLING-SECTION-REF: the renumber orphan ------------------------
    #
    # The ceiling is passed explicitly here. Production reads the module
    # constant; the fixture has three documents rather than 748 files, so it
    # asserts against 0 and proves the rule fires at all rather than proving a
    # particular slack.
    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        # THE CITATIONS BELOW ARE COMPOSED, NOT WRITTEN OUT, and that is not
        # style. A fixture string in the scanned form makes THIS FILE a citing
        # file: the rule counted its own test data and read 11 where 8 are. That
        # is the third time in this one file that a literal example got scanned
        # as a real citation, so the fixtures are built from parts and no
        # contiguous `docs/<DOC>.md §N` ever appears in the source.
        def cite(doc: str, number: str, tail: str = "") -> str:
            return "See `docs/" + doc + ".md` \u00a7" + number + tail + "\n"

        _write(d, "docs/GUIDE.md", cite("DETERMINISM", "1", " for the real one."))
        expect(
            "a qualified reference to a section that EXISTS is not dangling",
            check_section_refs(d, ceiling=0) == [],
        )

        # A sub-heading satisfies a sub-section citation. This is the assertion
        # that keeps the 62 false positives from coming back: check `##` only
        # and this case fails.
        _write(d, "docs/DETERMINISM.md", "## 1. Foo\nbody\n\n## 2. Bar\n### 2.4 Sub\nbody\n")
        _write(d, "docs/GUIDE.md", cite("DETERMINISM", "2.4", " \u2014 a sub-heading."))
        expect(
            "a citation of §2.4 is satisfied by a `### 2.4` sub-heading",
            check_section_refs(d, ceiling=0) == [],
        )

        # And now the orphan.
        _write(d, "docs/GUIDE.md", cite("PARITY-ROADMAP", "77", " which is gone."))
        reds = check_section_refs(d, ceiling=0)
        expect("a citation of a section that does not exist is caught", len(reds) == 1)
        expect(
            "the red names the citing file, its line and the missing section",
            reds
            and "docs/GUIDE.md:1" in reds[0]
            and "PARITY-ROADMAP §77" in reds[0],
        )
        expect(
            "the ceiling is a ceiling — the same orphan under a slack of 1 is green",
            check_section_refs(d, ceiling=1) == [],
        )

        # An UNQUALIFIED reference is deliberately not resolved: it names no
        # document, so the checker would have to guess which one §77 belongs to.
        _write(d, "docs/GUIDE.md", "See \u00a777, wherever that is.\n")
        expect(
            "a bare §77 naming no document is not scanned",
            check_section_refs(d, ceiling=0) == [],
        )

        # A citation from tracked SOURCE counts, not only from prose. Five of the
        # eight real dangling references live in .rs files.
        _write(d, "docs/GUIDE.md", "nothing here\n")
        _write(d, "crates/x/src/lib.rs", "// " + cite("DETERMINISM", "77"))
        reds = check_section_refs(d, ceiling=0)
        expect(
            "a dangling citation in a .rs file is caught too",
            len(reds) == 1 and "crates/x/src/lib.rs:1" in reds[0],
        )

    # --- `status`, poisoned in both directions --------------------------
    #
    # The whole hazard of adding an exemption is that the exemption becomes a
    # hole. So the four cases below assert, in order: that `reserved` buys
    # exactly the one exemption it is supposed to; that it buys nothing else;
    # that it expires; and that an unknown value is refused rather than
    # silently treated as one of the two known ones.

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(
            d,
            "docs/claims/PARITY-ROADMAP-9-held.toml",
            _claim("PARITY-ROADMAP", "9", "held") + 'status = "reserved"\n',
        )
        reds, n = check(d)
        expect(
            "a reserved claim with no heading is NOT CLAIM-NO-HEADING",
            not any(r.startswith("CLAIM-NO-HEADING") for r in reds),
        )
        expect("and the tree is otherwise green", reds == [])
        expect("the reserved claim is still counted", n == 4)

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        # The same number, once reserved and once written. If `reserved` were
        # a hole, this is the shape that would fall through it — and it is
        # exactly the §45/§51/§54/§57 shape live in the ref store today.
        _write(
            d,
            "docs/claims/PARITY-ROADMAP-9-held.toml",
            _claim("PARITY-ROADMAP", "9", "held") + 'status = "reserved"\n',
        )
        _write(
            d,
            "docs/claims/PARITY-ROADMAP-9-alsoheld.toml",
            _claim("PARITY-ROADMAP", "9", "alsoheld"),
        )
        reds, _ = check(d)
        dup = [r for r in reds if r.startswith("DUPLICATE-CLAIM")]
        expect("a reserved claim still collides — it is not a hole", len(dup) == 1)
        expect(
            "and the duplicate names both claimants",
            dup and "PARITY-ROADMAP-9-alsoheld.toml" in dup[0]
            and "PARITY-ROADMAP-9-held.toml" in dup[0],
        )

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        # The reservation has been honoured — the section is written — but the
        # status was never flipped. A registry that let this stand would be
        # reporting an unwritten section that exists.
        _write(d, "docs/PARITY-ROADMAP.md", "## 1. Baz\nbody\n\n## 9. Held\nbody\n")
        _write(
            d,
            "docs/claims/PARITY-ROADMAP-9-held.toml",
            _claim("PARITY-ROADMAP", "9", "held") + 'status = "reserved"\n',
        )
        reds, _ = check(d)
        rbw = [r for r in reds if r.startswith("RESERVED-BUT-WRITTEN")]
        expect("a reservation whose section now exists is caught", len(rbw) == 1)
        expect(
            "and the red says what to do about it",
            rbw and 'status = "written"' in rbw[0],
        )

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        _write(
            d,
            "docs/claims/PARITY-ROADMAP-9-held.toml",
            _claim("PARITY-ROADMAP", "9", "held") + 'status = "pending"\n',
        )
        reds, n = check(d)
        expect(
            "an unrecognised status is MALFORMED, not silently accepted",
            any(r.startswith("MALFORMED") and "pending" in r for r in reds),
        )
        expect("and the malformed claim is not counted as valid", n == 3)

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        _base_tree(d)
        reds, n = check(d)
        expect(
            "a claim file with no status at all is still valid (defaults written)",
            reds == [] and n == 3,
        )

    print("SELFTEST GREEN" if ok else "SELFTEST RED")
    return 0 if ok else 1


def main() -> int:
    if "--selftest" in sys.argv:
        return _selftest()

    reds, n = check(ROOT)
    if reds:
        for r in reds:
            print(f"check-doc-claims: {r}", file=sys.stderr)
        return 1
    print(
        f"check-doc-claims: {n} claims registered across {len(DOCS)} docs, "
        "0 collisions, 0 drift"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
