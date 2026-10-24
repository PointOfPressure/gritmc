#!/usr/bin/env python3
"""Git merge driver for a doc that is append-only IN PART: `docs/WORKLIST.md`,
`docs/PARITY-ROADMAP.md`, `ci/figures.toml`, `ci/checks/gameplay-falsifiable.toml`,
`ci/checks/gameplay-baseline.toml`, `ci/t0.sh`.

# WHY THIS EXISTS, AND WHY `merge=appendonly` IS THE WRONG ANSWER HERE

`ci/merge-appendonly.py` handles a file where *every* edit is a whole new tail
appended after the last one — `docs/DETERMINISM.md`. It takes its fast path
only when both sides are exact byte-prefix extensions of the base, and refuses
everything else. That refusal is the reason it is safe, and it is not weakened
here: that driver and that file are untouched by this one.

The three files above are a different shape and the difference is the whole
design problem. Measured on 2026-08-16 over the last 40 commits touching
`docs/WORKLIST.md`: **36 of them add exactly one row to the COLD START INDEX
table** at the same anchor, and 4 also rewrite prose inside a `## NOW` block.
So the file is neither append-only nor freely mergeable. It is one small
append-only *region* embedded in a document that is rewritten in place
everywhere else. `docs/PARITY-ROADMAP.md` is the same with the region at the
tail (every branch appends a new `## N.` section at EOF); `ci/figures.toml` is
the same with the region being its `[[figure]]` body.

`ci/t0.sh` is the same defect, one file wider than the append-at-EOF case: six
branches diffed 2026-08-17 (`ci-required-checks-ratchet`, `lockpin-plugin-api`,
`gameplay-corpus-checks-falsify`, `t0-test-target-coverage`,
`regiondiff-live-fixture`, `worldgen-appendonly-3way`) touched it with **zero
deletions between them** — every changed line a `+` — but each new check block
lands next to the existing one it is topically closest to, not at a shared
tail. So its region spans the whole executable body rather than one bounded
table or a `to-eof` tail, from just after `cd` to just before the closing
sentinel. A real edit inside that span still conflicts exactly as it would
outside one — none of the three resolutions below can keep both sides of a
rewritten line — so the width of the region does not weaken what it refuses,
it only widens where a clean append is recognised as one. `ci/t1.sh` was
checked the same day and is NOT this shape: 58 lines total, its own header
says the order between its two `cargo test` calls is LOAD-BEARING, and 3 of
its last 5 commits touching it carry deletions — real edits to existing
lines, not appends. It carries no attribute here as a result; re-check if
that history changes.

Marking the whole file `merge=appendonly` would refuse every merge, because
neither side is ever a byte-prefix extension. Marking it `merge=union` is the
defect this project has already paid for once — see `.gitattributes` and
`ci/t0.sh` — and the damage is *still legible in the tree*: the COLD START
INDEX today carries `ci-unfalsifiable-root` twice with **"10 of 84"** in one
row and **"4 of 84"** in the other, `ci-figures-ledger` twice, and
`useblock-port-the-17` three times. A branch replaced its own row, the union
kept both, and two contradictory figures now sit in the ledger with nothing to
say which is current. That is exactly the failure a conflict would have made
visible.

# THE DESIGN: REGIONS DECLARED IN THE FILE, ADD/ADD RESOLVED ONLY INSIDE THEM

A file declares its own append-only regions with marker lines. The markers
live in the file rather than in a config table for the same reason
`data/vanilla` is copied rather than shared: a second place to declare
something is a place for it to drift, and a branch that moves a section
carries its own declaration with it.

    <!-- MERGE-REGION BEGIN cold-start-index key=first-cell check=unique-first-cell -->
    ...rows...
    <!-- MERGE-REGION END cold-start-index -->

    # MERGE-REGION BEGIN figures to-eof check=toml-figures

The marker is matched anywhere in a line, so the host file's own comment
syntax carries it — HTML comments in Markdown, `#` in TOML — and it is inert
to every reader: `grep -c '^## NOW'` still returns 3, `ci/now.sh`'s awk is
unaffected, and tomllib sees a comment.

Attributes on BEGIN:

* `to-eof`   — the region runs to end of file and has no END marker. This is
               the right shape for a tail append: it needs no cooperation
               from the appending branch, because EOF is where it already
               writes.
* `key=K`    — the region is a set of keyed rows. See KEYED ROWS below.
* `check=X`  — a post-condition, run only when this driver auto-resolved
               something. See POST-CONDITIONS below.

The merge itself:

1. `git merge-file -p --diff3` the three sides — git's own 3-way engine, so
   behaviour outside a region is git's default behaviour, not a
   reimplementation of it. Clean merge, nothing to do.
2. Otherwise walk the diff3 output, tracking which declared region (if any)
   each conflict hunk falls in. Structure is read from the **ours** side,
   which during a rebase is upstream and during a merge is the target branch
   — i.e. the side that carries the markers.
3. A conflict hunk OUTSIDE every region keeps its markers, always. Inside a
   region, three resolutions are tried in order and each is a finer-grained
   3-way answer rather than a policy:
     a. **base slice EMPTY** — neither side changed existing text, both merely
        inserted at the same anchor. Result is ours' insertion then theirs',
        each internally in its own order. Same answer
        `ci/merge-appendonly.py` gives for a tail, per-hunk instead of
        per-file.
     b. **keyed rows** (see below), if the region declares a key.
     c. **one side left the base contiguous** (see below).
   Anything none of the three resolves keeps standard
   `<<<<<<<`/`|||||||`/`=======`/`>>>>>>>` markers and the exit code is
   non-zero, so git reports the path unmerged and a human resolves it exactly
   as before this driver existed.

**Not one of (a), (b) or (c) can keep both sides of a rewritten line**, which
is the guarantee the `## NOW` blocks need — and for `## NOW` it holds twice
over, because `## NOW` is outside every declared region and never reaches the
test at all. `docs/WORKLIST.md` declares exactly one region and the only thing
inside it is the COLD START INDEX table body.

# KEYED ROWS, AND AN UNTOUCHED CONTIGUOUS BASE

Both exist for the same reason: git's hunks are line ranges, so an edit and an
insertion that do not overlap get conflated whenever they are ADJACENT, and
that adjacency is the normal case here — every branch writes at the same
anchor. Both were added against a measured branch, and both keep every
refusal:

* `key=first-cell` merges a hunk of table rows per key. A key only one side
  changed takes that side; a key BOTH changed differently, a key either side
  deleted, and a key both sides added differently all still conflict. Measured
  on `t0-assertion-floor`: main inserted two rows above that branch's own row
  while the branch replaced it, exactly as `docs/AGENT-BRIEF.md` instructs.
  `key=toml-key` is the same rule for a bare `name = "value"` assignment
  instead of a Markdown table row — `ci/checks/gameplay-baseline.toml`'s
  shape. It does NOT reach `ci/checks/gameplay-falsifiable.toml`'s
  `[[scenario]]` rows, which are multi-line stanzas rather than one row per
  line: two branches each re-sweeping the SAME existing scenario differently
  is a real disagreement about what the evidence says and must still conflict
  by line, the same as it would with no driver at all — only a clean new
  stanza inserted at the tail, or one genuinely untouched by the other side,
  reaches the append rule below.
* the contiguity rule handles the same shape without a key: if one side
  contains the whole base slice contiguously and exactly once, that side
  edited nothing and only inserted around it, so the other side's version of
  the base is taken and the insertions stay where they were. Measured on
  `gameplay-block-schemas`: main appended 381 lines directly above that
  branch's own heading, which the branch had renumbered `## 44.` -> `## 48.`
  because main claimed 44 first.

# WHAT THIS DRIVER WILL NOT DO, DELIBERATELY

* It will not keep both sides of a rewritten section. A driver that silently
  kept both would be strictly worse than the conflict it replaced, because
  the conflict is visible and the stale figure is not.
* It will not resolve an add/add outside a declared region. Two branches
  appending different paragraphs at the same point in `## NOW` is a place
  where a human should look, and the file says so by not declaring a region
  there.
* It will not resolve anything when the markers are malformed. It degrades to
  git's default merge and says so on stderr. The malformation itself is
  caught where a check belongs — `--check`, wired into
  `ci/setup-merge-drivers.sh` and so into `ci/t0.sh` — rather than at merge
  time where nobody reads stderr.
* It does not read structure from `theirs`. Merging main *into* a branch,
  before that branch has the markers, gets git's default behaviour. Safe
  direction: it refuses rather than guesses.

# POST-CONDITIONS — the guard against this rotting into a union

They run **only when a hunk was auto-resolved**, so this driver can never
refuse a merge that git's default driver would have completed. Each takes the
form "the merged file may not assert a key more times than the side that
asserts it most", which is precisely what a union violates and a legitimate
concurrent append does not:

* `unique-first-cell`      — Markdown table rows keyed on their first cell.
                             Catches the duplicated-branch-row defect above.
* `unique-bullet-line`     — `docs/FINDINGS.md`'s bullets, keyed on the
                             bullet's own first line.
* `toml-buckets`           — `ci/checks/test-targets.toml`: the merged file
                             parses, its `[[bucket]]` ids are unique, and no
                             target is listed in two buckets.
* `unique-heading-number`  — `## N.` section numbers. `ci/check-doc-claims.py`
                             refuses a duplicate at gate time; this refuses it
                             at merge time, which is a whole gate cheaper.
* `toml-figures`           — the merged file must parse as TOML and its
                             `[[figure]]` ids must be unique.
* `toml-scenarios`         — same shape, for `ci/checks/gameplay-falsifiable.toml`'s
                             `[[scenario]]` names.
* `unique-toml-key`        — `ci/checks/gameplay-baseline.toml`'s plain
                             `key = "verdict"` rows are one table, not an
                             array of tables, so TOML itself already refuses a
                             repeated key; this just runs the parse.
* `bash-syntax`            — `ci/t0.sh`'s region has no key at all (a check
                             block is not named anywhere the merge can read);
                             `bash -n` on the whole merged file is the one
                             cheap, unconditional guarantee available for
                             executable code, where a wrong merge breaks the
                             gate for every branch, not just its own author.

A failed post-condition is a refusal: conflict markers over the whole file and
a non-zero exit, same as any other genuine disagreement.

# PROTOCOL

Invoked by git through `merge.docregions.driver` (`ci/setup-merge-drivers.sh`
sets it, per checkout, because `.gitattributes` cannot carry a local command):

    python3 ci/merge-docregions.py %O %A %B [%P]

`%O` base, `%A` ours, `%B` theirs, `%P` the real pathname (optional, used only
in messages). The result is written back into `%A` and the exit code says
clean (`0`) or conflict (non-zero).

Also runnable as a checker, which needs no git and no merge:

    python3 ci/merge-docregions.py --check docs/WORKLIST.md ...
"""

from __future__ import annotations

import collections
import os
import re
import subprocess
import sys

# Long markers so that a line of body text can never be mistaken for one. A
# Markdown setext underline is a real line of `=======`; a 32-character run is
# not something this project's docs contain, and `--check`-style paranoia here
# is cheap. Standard 7-character markers are what gets WRITTEN OUT on a
# refusal, because that is what a human and every other tool expect to see.
MARK = 32
L_OURS, L_BASE, L_THEIRS = "MERGE-REGION-OURS", "MERGE-REGION-BASE", "MERGE-REGION-THEIRS"

COMMENT_CLOSERS = ("-->", "*/", "#}", "}}")

MARKER_RE = re.compile(
    r"MERGE-REGION\s+(?P<kind>BEGIN|END)\s+(?P<name>[A-Za-z0-9._-]+)\s*(?P<attrs>.*?)\s*$"
)
ROW_RE = re.compile(r"^\s*\|(?P<first>[^|]*)\|")
SEP_RE = re.compile(r"^\s*\|[\s:-]+\|")
BULLET_RE = re.compile(r"^[ \t]*[*-][ \t]+(.*\S)[ \t]*$")
HEADING_RE = re.compile(r"^##\s+(\d+)\.")


class Malformed(Exception):
    pass


class Region:
    __slots__ = ("name", "to_eof", "check", "key")

    def __init__(self, name: str, attrs: str):
        self.name = name
        toks = attrs.split()
        self.to_eof = "to-eof" in toks
        self.check = None
        self.key = None
        for t in toks:
            if t.startswith("check="):
                self.check = t[len("check="):]
            elif t.startswith("key="):
                self.key = t[len("key="):]
                if self.key not in KEYFNS:
                    raise Malformed(f"region {name!r}: unknown key {self.key!r}")
            elif t in ("to-eof",) or t in COMMENT_CLOSERS:
                # A closer is whatever terminates the HOST file's comment
                # syntax — `-->` in Markdown. The marker is deliberately
                # matched anywhere in a line so the host's own comment carries
                # it, which means the closer lands in the attribute tail.
                continue
            else:
                raise Malformed(f"region {name!r}: unknown attribute {t!r}")
        if self.check is not None and self.check not in POSTCONDITIONS:
            raise Malformed(f"region {name!r}: unknown check {self.check!r}")


def scan_regions(lines: list[str]) -> list[Region]:
    """Validate marker structure over a whole file. Raises Malformed."""
    out: list[Region] = []
    seen: set[str] = set()
    open_r: Region | None = None
    for i, ln in enumerate(lines, 1):
        m = MARKER_RE.search(ln)
        if not m:
            continue
        name = m.group("name")
        if m.group("kind") == "BEGIN":
            if open_r is not None:
                raise Malformed(
                    f"line {i}: BEGIN {name} while {open_r.name} is still open — "
                    "regions do not nest"
                )
            if name in seen:
                raise Malformed(f"line {i}: region {name!r} declared twice")
            seen.add(name)
            r = Region(name, m.group("attrs"))
            out.append(r)
            if not r.to_eof:
                open_r = r
        else:
            if open_r is None or open_r.name != name:
                raise Malformed(
                    f"line {i}: END {name} with no matching open BEGIN"
                )
            open_r = None
    if open_r is not None:
        raise Malformed(
            f"region {open_r.name!r} is never closed and is not declared `to-eof` — "
            "a lost END marker must not silently become an open region"
        )
    return out


# --- diff3 parsing ----------------------------------------------------------

def _mk(ch: str, label: str = "") -> str:
    return ch * MARK + (" " + label if label else "")


def parse_diff3(text: str) -> list:
    """-> list of ('text', lines) | ('conflict', ours, base, theirs)."""
    # No trailing-empty pop: `"a\nb\n".split("\n")` keeps a final `""` and
    # `"\n".join` puts the newline back, so a file's final newline survives a
    # round trip. Dropping it here would strip the trailing newline from every
    # merged file, which `git diff` reports as `\ No newline at end of file`
    # on a merge nobody edited by hand.
    lines = text.split("\n")
    open_m, base_m, sep_m, close_m = (
        _mk("<", L_OURS), _mk("|", L_BASE), _mk("="), _mk(">", L_THEIRS),
    )
    out, buf, i = [], [], 0
    while i < len(lines):
        if lines[i] == open_m:
            if buf:
                out.append(("text", buf))
                buf = []
            i += 1
            ours: list[str] = []
            while i < len(lines) and lines[i] != base_m:
                ours.append(lines[i]); i += 1
            if i >= len(lines):
                raise Malformed("diff3 output truncated: no base marker")
            i += 1
            base: list[str] = []
            while i < len(lines) and lines[i] != sep_m:
                base.append(lines[i]); i += 1
            if i >= len(lines):
                raise Malformed("diff3 output truncated: no separator")
            i += 1
            theirs: list[str] = []
            while i < len(lines) and lines[i] != close_m:
                theirs.append(lines[i]); i += 1
            if i >= len(lines):
                raise Malformed("diff3 output truncated: no closing marker")
            i += 1
            out.append(("conflict", ours, base, theirs))
        else:
            buf.append(lines[i]); i += 1
    if buf:
        out.append(("text", buf))
    return out


def render_conflict(ours: list[str], base: list[str], theirs: list[str]) -> list[str]:
    """Standard 7-character markers — what a human and every other tool read."""
    return (
        ["<<<<<<< ours"] + ours
        + ["||||||| base"] + base
        + ["======="] + theirs
        + [">>>>>>> theirs"]
    )


# --- keyed rows: the "replace your own row" case ----------------------------
#
# `docs/AGENT-BRIEF.md` tells a branch to REPLACE its row in the COLD START
# INDEX if it already has one. That is an edit to existing text, so the
# base-slice-empty rule above refuses it — correctly for a doc with no notion
# of a row identity, and needlessly here, because a table row HAS one. The
# case that made this necessary, measured 2026-08-16 on `t0-assertion-floor`:
# main inserted two new rows ABOVE that branch's own row while the branch
# rewrote it. Nothing overlaps; git conflates them into one hunk only because
# the lines are adjacent.
#
# So a region may declare `key=first-cell`, and a hunk whose every line on
# every side is a keyed row is merged per key instead of per line:
#
#   * a key only ours changed -> ours, a key only theirs changed -> theirs
#   * a key BOTH changed, differently                            -> CONFLICT
#   * a key either side DELETED                                  -> CONFLICT
#   * a key both sides ADDED with different content              -> CONFLICT
#
# The three refusals are the point. This is a finer-grained merge, not a more
# permissive one: every genuine disagreement about a row still conflicts, and
# a key can never end up in the output twice, which is what the union did.

def key_first_cell(line: str) -> str | None:
    if SEP_RE.match(line):
        return None
    m = ROW_RE.match(line)
    if not m:
        return None
    k = m.group("first").strip().strip("`").strip()
    return k or None


# `key=toml-key` is `key=first-cell`'s counterpart for a region whose rows are
# bare TOML assignments — `ci/checks/gameplay-baseline.toml`'s `name =
# "verdict"` lines — rather than Markdown table rows. Comments and blank lines
# return None exactly as a separator row does, so a comment sitting between
# two rows (every dated block in that file opens with one) makes the hunk it
# is part of fall through keyed resolution to the plain append rule instead —
# still correct, just coarser, and never a silent union.
TOML_KEY_RE = re.compile(r"^([A-Za-z0-9_.-]+)\s*=")


def key_toml_key(line: str) -> str | None:
    if line.lstrip().startswith("#"):
        return None
    m = TOML_KEY_RE.match(line)
    return m.group(1) if m else None


KEYFNS = {"first-cell": key_first_cell, "toml-key": key_toml_key}


def keyed_resolve(ours, base, theirs, keyfn):
    """-> merged lines, or None meaning 'not keyable / genuine conflict'."""
    maps = []
    for side in (ours, base, theirs):
        m = {}
        for ln in side:
            k = keyfn(ln)
            if k is None or k in m:
                # An unkeyable line (a marker, a separator, prose) or a key
                # repeated inside one slice. Either way this hunk is not a
                # clean set of rows and gets no special treatment.
                return None
            m[k] = ln
        maps.append(m)
    om, bm, tm = maps
    out = []
    for ln in ours:
        k = keyfn(ln)
        if k in bm:
            b, o = bm[k], ln
            if k not in tm:
                return None                      # theirs deleted a row
            t = tm[k]
            if o == b:
                out.append(t)                    # only theirs changed (or neither)
            elif t == b or t == o:
                out.append(o)                    # only ours changed, or same change
            else:
                return None                      # both changed, differently
        else:
            if k in tm and tm[k] != ln:
                return None                      # both ADDED the same key, differently
            out.append(ln)
    for k in bm:
        if k not in om:
            return None                          # ours deleted a row
    for ln in theirs:
        k = keyfn(ln)
        if k not in bm and k not in om:
            out.append(ln)                       # theirs' new rows
    return out


# --- an untouched base, insertions around it --------------------------------
#
# The keyed rule above only reaches a hunk that is entirely table rows. The
# same false conflict happens one level up and needs no key. Measured
# 2026-08-16 on `gameplay-block-schemas`: main appended 381 lines of new
# sections, and immediately below them sat that branch's own `## 44.` heading,
# which the branch had RENUMBERED to `## 48.` because main claimed 44 first.
# So `ours` is [381 new lines] + [the base heading, untouched] while `theirs`
# is [the base heading, renumbered]. Git conflates the two because they are
# adjacent, but ours changed nothing in the base at all.
#
# The rule, and it is a plain 3-way result rather than a policy: **if one side
# contains the whole base slice contiguously and exactly once, that side edited
# nothing — it only inserted around it — so take the other side's version of
# the base and keep this side's insertions where they were.**
#
# Uniqueness is load-bearing. A base slice appearing twice in a side gives no
# unambiguous place to splice, and this refuses rather than picks. If NEITHER
# side left the base contiguous, both rewrote it, and that is a real
# disagreement.

def _unique_at(hay: list[str], needle: list[str]) -> int | None:
    n = len(needle)
    hits = [i for i in range(len(hay) - n + 1) if hay[i:i + n] == needle]
    return hits[0] if len(hits) == 1 else None


def contiguous_resolve(ours, base, theirs):
    """-> merged lines, or None meaning 'both sides rewrote the base'."""
    if not base:
        return None
    io, it = _unique_at(ours, base), _unique_at(theirs, base)
    n = len(base)
    if io is not None and it is None:
        return ours[:io] + theirs + ours[io + n:]
    if it is not None and io is None:
        return theirs[:it] + ours + theirs[it + n:]
    if io is not None and it is not None:
        # Both only inserted, at different points around an untouched base.
        # Ours' insertions first on each side of it, matching the order the
        # pure-append rule uses.
        return ours[:io] + theirs[:it] + base + theirs[it + n:] + ours[io + n:]
    return None


def split_trailing_markers(ours: list[str], name: str) -> tuple[list[str], list[str]]:
    """Peel an END marker (and the blank lines glued to it) off the end of a
    resolved slice, so a row appended at the very bottom of a bounded region
    lands INSIDE the region rather than below its closing marker. Only ever
    fires during the transition, while `theirs` predates the markers: once
    both sides carry them the markers are base content and never appear in a
    conflict slice at all."""
    cut = len(ours)
    saw_marker = False
    while cut > 0:
        ln = ours[cut - 1]
        m = MARKER_RE.search(ln)
        if m and m.group("kind") == "END" and m.group("name") == name:
            saw_marker = True
            cut -= 1
        elif ln.strip() == "":
            cut -= 1
        else:
            break
    if not saw_marker:
        return ours, []
    return ours[:cut], ours[cut:]


# --- post-conditions --------------------------------------------------------

def _table_keys(lines: list[str]) -> collections.Counter:
    c: collections.Counter = collections.Counter()
    for ln in lines:
        if SEP_RE.match(ln):
            continue
        m = ROW_RE.match(ln)
        if m:
            k = m.group("first").strip().strip("`").strip()
            if k:
                c[k] += 1
    return c


# `docs/FINDINGS.md`'s rows are Markdown bullets — one finding each, the first
# line carrying the date and the opening clause, continuation lines indented.
# The key is that first line, normalised for whitespace only: two lanes writing
# a byte-identical opening clause on the same day is not a thing that happens,
# and if it did, `_multiplicity` still allows it as long as the merge does not
# assert it MORE often than the side that asserts it most. What it refuses is
# the union defect — a rewritten finding kept beside its rewrite.
def _bullet_keys(lines: list[str]) -> collections.Counter:
    c: collections.Counter = collections.Counter()
    for ln in lines:
        m = BULLET_RE.match(ln)
        if m:
            k = " ".join(m.group(1).split())
            if k:
                c[k] += 1
    return c


def _heading_keys(lines: list[str]) -> collections.Counter:
    c: collections.Counter = collections.Counter()
    for ln in lines:
        m = HEADING_RE.match(ln)
        if m:
            c[m.group(1)] += 1
    return c


def region_body(lines: list[str], name: str) -> list[str]:
    """The lines between this region's markers, or the whole side if it does
    not declare the region — which is the case for a `theirs` that predates
    the markers. Scoping matters: `docs/WORKLIST.md` holds several OTHER
    tables whose first cell legitimately repeats (`chest-opens-a-screen`,
    `chat-commands`, the scenario tables), and counting keys file-wide would
    refuse a merge where one side adds a row to the index and the other adds
    an unrelated row with the same first cell to a scenario table. That is a
    false refusal, and a post-condition that fires on innocent input is the
    same defect class as a check that cannot fire at all."""
    out, inside = [], False
    for ln in lines:
        m = MARKER_RE.search(ln)
        if m and m.group("name") == name:
            if m.group("kind") == "BEGIN":
                inside = True
                continue
            inside = False
            continue
        if inside:
            out.append(ln)
    return out if out else lines


def _multiplicity(counter_fn, result, ours, theirs, what):
    r, o, t = counter_fn(result), counter_fn(ours), counter_fn(theirs)
    bad = [k for k, n in r.items() if n > max(o.get(k, 0), t.get(k, 0))]
    if bad:
        return (
            f"{what} appear more often in the merge than in either side alone: "
            + ", ".join(f"{k!r} x{r[k]} (ours x{o.get(k, 0)}, theirs x{t.get(k, 0)})"
                        for k in sorted(bad))
            + " — that is a union keeping a replaced entry beside its replacement, "
              "which is the defect this driver exists to refuse"
        )
    return None


def pc_unique_first_cell(result, ours, theirs, region):
    return _multiplicity(_table_keys, region_body(result, region),
                         region_body(ours, region), region_body(theirs, region),
                         "table rows")


# `ci/checks/test-targets.toml` is `ci/t0.sh`'s shape in TOML: branches add a
# whole new `[[bucket]]` stanza next to the bucket it is topically closest to
# rather than at a shared tail — measured 2026-09-05, the six hunks on
# `light-engine-2` start at lines 182, 250, 274, 287, 628 and 709 — so its
# region is the whole body and the post-condition carries the weight. Two
# counts, because a bucket and a target are different things to duplicate: a
# repeated `id` is a union keeping a replaced bucket beside its replacement, and
# a target listed in two buckets is a target running in two gates, which the
# ledger's own reader (`ci/check-test-targets.py`) is entitled to assume cannot
# happen.
def pc_toml_buckets(result, ours, theirs, region):
    try:
        import tomllib
    except ImportError:  # pragma: no cover - python < 3.11
        return None
    def parse(lines, which):
        try:
            return tomllib.loads("\n".join(lines)), None
        except Exception as e:
            return None, f"the merged file does not parse as TOML: {e}"
    doc, err = parse(result, "result")
    if err:
        return err
    buckets = doc.get("bucket", [])
    ids = [b.get("id") for b in buckets]
    dup = sorted({i for i, n in collections.Counter(ids).items() if n > 1})
    if dup:
        return f"duplicate [[bucket]] ids after the merge: {dup}"
    members: collections.Counter = collections.Counter()
    for b in buckets:
        for m in b.get("members", []):
            members[m] += 1
    dupm = sorted({m for m, n in members.items() if n > 1})
    if dupm:
        return ("targets listed in more than one bucket after the merge: "
                + ", ".join(repr(m) for m in dupm)
                + " — a target that runs in two gates is a ledger the reader "
                  "cannot trust, and a union is how it gets there")
    return None


def pc_unique_bullet_line(result, ours, theirs, region):
    return _multiplicity(_bullet_keys, region_body(result, region),
                         region_body(ours, region), region_body(theirs, region),
                         "finding bullets")


def pc_unique_heading_number(result, ours, theirs, region):
    return _multiplicity(_heading_keys, region_body(result, region),
                         region_body(ours, region), region_body(theirs, region),
                         "`## N.` section numbers")


def pc_toml_figures(result, ours, theirs, region):
    try:
        import tomllib
    except ImportError:  # pragma: no cover - python < 3.11
        return None
    try:
        doc = tomllib.loads("\n".join(result))
    except Exception as e:
        return f"the merged file does not parse as TOML: {e}"
    ids = [f.get("id") for f in doc.get("figure", [])]
    dup = sorted({i for i, n in collections.Counter(ids).items() if n > 1})
    if dup:
        return f"duplicate [[figure]] ids after the merge: {dup}"
    return None


# `ci/checks/gameplay-falsifiable.toml`'s rows are `[[scenario]]` — an array
# of tables, the same shape as `[[figure]]` — so a duplicate `name` is legal
# TOML and needs the same manual count `pc_toml_figures` does. It parses the
# WHOLE merged file rather than just the region for the same reason that one
# does: the region is declared `to-eof` with no key, so nothing outside a
# resolved hunk changed, and the file has no non-TOML content mixed in to trip
# a parser scoped to less than everything.
def pc_toml_scenarios(result, ours, theirs, region):
    try:
        import tomllib
    except ImportError:  # pragma: no cover - python < 3.11
        return None
    try:
        doc = tomllib.loads("\n".join(result))
    except Exception as e:
        return f"the merged file does not parse as TOML: {e}"
    names = [s.get("name") for s in doc.get("scenario", [])]
    dup = sorted({n for n, c in collections.Counter(names).items() if c > 1})
    if dup:
        return f"duplicate [[scenario]] names after the merge: {dup}"
    return None


# `ci/checks/gameplay-baseline.toml`'s rows are plain `key = "verdict"`
# assignments in one table (`[expect]`), which is a shape TOML itself already
# refuses to parse twice — `tomllib` raises on a repeated key in the same
# table, so the parse attempt alone is the duplicate check; there is nothing
# left to count by hand the way `pc_toml_figures` has to for an array of
# tables.
def pc_unique_toml_key(result, ours, theirs, region):
    try:
        import tomllib
    except ImportError:  # pragma: no cover - python < 3.11
        return None
    try:
        tomllib.loads("\n".join(result))
    except Exception as e:
        return f"the merged file does not parse as TOML: {e}"
    return None


# `ci/t0.sh`'s region is executable code, not a doc or a data ledger, and a
# WRONG merge there breaks the gate for every branch, not just the one that
# wrote it — the bar this file's docstring already sets for "when to trust the
# append rule at all". `unique-first-cell`/`unique-heading-number`/`toml-*`
# each catch a UNION (a key surviving twice); nothing about this region has a
# key, because a check block is not named anywhere the merge can read. What
# CAN be checked cheaply and unconditionally is the one property every other
# guarantee here rests on transitively: **the result still parses as bash.**
# `bash -n` reads the WHOLE merged file from stdin — not just the region body,
# the same scope `pc_toml_figures` uses on `ci/figures.toml` for the same
# reason: a script has no notion of "the rest is somebody else's problem", one
# bad append anywhere breaks every line after it. This does not, and cannot,
# prove the NEW check is correct — only that appending it did not corrupt the
# file structurally. `ci/merge-appendonly-falsify.sh` case 27 below is the
# guard's own red-when-broken evidence: a union that quietly drops a line's
# closing quote must fail `bash -n` even though it fails no OTHER check here.
def pc_bash_syntax(result, ours, theirs, region):
    try:
        proc = subprocess.run(
            ["bash", "-n"], input="\n".join(result), text=True,
            capture_output=True, timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        return f"could not run `bash -n` on the merged file: {e}"
    if proc.returncode != 0:
        return f"the merged file does not parse as bash: {proc.stderr.strip()}"
    return None


POSTCONDITIONS = {
    "unique-first-cell": pc_unique_first_cell,
    "unique-bullet-line": pc_unique_bullet_line,
    "toml-buckets": pc_toml_buckets,
    "unique-heading-number": pc_unique_heading_number,
    "toml-figures": pc_toml_figures,
    "toml-scenarios": pc_toml_scenarios,
    "unique-toml-key": pc_unique_toml_key,
    "bash-syntax": pc_bash_syntax,
}


# --- the merge ---------------------------------------------------------------

def git_merge_file(base: str, ours: str, theirs: str) -> tuple[str, int]:
    p = subprocess.run(
        ["git", "merge-file", "-p", "--diff3", f"--marker-size={MARK}",
         "-L", L_OURS, "-L", L_BASE, "-L", L_THEIRS, ours, base, theirs],
        capture_output=True,
    )
    if p.returncode < 0 or p.returncode > 127:
        raise Malformed(f"git merge-file failed: {p.stderr.decode('utf-8', 'replace')}")
    return p.stdout.decode("utf-8", "surrogateescape"), p.returncode


def merge_text(base: str, ours: str, theirs: str, path: str) -> tuple[str, bool, list[str]]:
    """-> (merged text, clean, notes). Reads the three sides from FILES."""
    notes: list[str] = []
    merged, rc = git_merge_file(base, ours, theirs)
    if rc == 0:
        return merged, True, notes

    ours_lines = open(ours, encoding="utf-8", errors="surrogateescape").read().split("\n")
    theirs_lines = open(theirs, encoding="utf-8", errors="surrogateescape").read().split("\n")

    try:
        scan_regions(ours_lines)
    except Malformed as e:
        notes.append(
            f"{path}: MERGE-REGION markers on the ours side are malformed ({e}); "
            "falling back to git's default merge and resolving nothing. "
            "`ci/merge-docregions.py --check` in T0 is where this is meant to be caught."
        )
        # Re-rendered with standard 7-character markers: a degraded merge must
        # be indistinguishable from any other conflict to git, to a human and
        # to every tool that greps for one.
        return merged_with_whole_file_conflict(merged), False, notes

    items = parse_diff3(merged)
    out: list[str] = []
    open_r: Region | None = None
    resolved = 0
    still_conflicted = 0
    checks: set[tuple[str, str]] = set()

    def feed(lines: list[str]) -> None:
        nonlocal open_r
        for ln in lines:
            m = MARKER_RE.search(ln)
            if not m:
                continue
            if m.group("kind") == "BEGIN":
                try:
                    r = Region(m.group("name"), m.group("attrs"))
                except Malformed:
                    open_r = None
                    continue
                # Both kinds are "open" for the purpose of deciding hunks; a
                # `to-eof` region simply never meets an END that closes it.
                open_r = r
            else:
                if open_r is not None and open_r.name == m.group("name"):
                    open_r = None

    for item in items:
        if item[0] == "text":
            out.extend(item[1])
            feed(item[1])
            continue
        _, o, b, t = item
        pure_add = not b and o and t
        keyed = None
        if open_r is not None and not pure_add:
            if open_r.key is not None:
                keyed = keyed_resolve(o, b, t, KEYFNS[open_r.key])
            if keyed is None:
                keyed = contiguous_resolve(o, b, t)
        if open_r is not None and (pure_add or keyed is not None):
            if keyed is not None:
                out.extend(keyed)
            else:
                body, trailing = split_trailing_markers(o, open_r.name)
                out.extend(body)
                out.extend(t)
                out.extend(trailing)
            resolved += 1
            if open_r.check:
                checks.add((open_r.check, open_r.name))
        else:
            out.extend(render_conflict(o, b, t))
            still_conflicted += 1
        feed(o)

    if still_conflicted:
        why = []
        if resolved:
            why.append(f"{resolved} pure-append hunk(s) resolved inside a declared region")
        why.append(f"{still_conflicted} hunk(s) left conflicted")
        notes.append(f"{path}: {', '.join(why)} — a hunk that edits existing text, or "
                     "one outside every declared append region, is a real disagreement "
                     "and is handed to a human unchanged")
        return "\n".join(out), False, notes

    result = out
    for name, region in sorted(checks):
        problem = POSTCONDITIONS[name](result, ours_lines, theirs_lines, region)
        if problem is not None:
            notes.append(f"{path}: post-condition `{name}` REFUSED — {problem}")
            return merged_with_whole_file_conflict(merged), False, notes

    notes.append(f"{path}: {resolved} pure-append hunk(s) resolved inside a declared "
                 f"append region; 0 left conflicted")
    return "\n".join(result), True, notes


def merged_with_whole_file_conflict(diff3_text: str) -> str:
    """Re-render the long-marker diff3 output with the standard 7-character
    markers, so a refusal looks exactly like any other conflict."""
    items = parse_diff3(diff3_text)
    out: list[str] = []
    for item in items:
        if item[0] == "text":
            out.extend(item[1])
        else:
            out.extend(render_conflict(item[1], item[2], item[3]))
    return "\n".join(out)


# --- entry points ------------------------------------------------------------

def do_check(paths: list[str]) -> int:
    # A CHECK OVER NOTHING IS NOT A CHECK — 2026-08-18.
    #
    # This used to `for p in paths` and return 0, so `--check` with no file
    # arguments iterated over an empty list and exited GREEN having read
    # nothing. That is this project's central defect class sitting in the guard
    # for the merge machinery: `ci/t0.sh` line 188 treats a zero exit here as
    # "no marker was lost", and a caller that lost its file list would have got
    # that answer without a word.
    #
    # It is not hypothetical in shape. The list is passed as an UNQUOTED
    # `$REGION_FILES` from ci/setup-merge-drivers.sh (lines 101 and 143) —
    # unquoted because it must word-split — and an unquoted variable that goes
    # empty contributes no arguments at all rather than an empty one. Today it
    # is a literal and cannot go empty; the day it is derived from
    # `.gitattributes` (which is the obvious next refactor, and is exactly what
    # keeps the two lists from drifting) it can.
    #
    # Found while running `--check` by hand with no arguments and getting a
    # green over a file whose END marker had just been deleted.
    if not paths:
        print("merge-docregions --check REFUSED — no files given. Checking zero "
              "files and exiting 0 would report 'no marker was lost' having read "
              "nothing; see ci/setup-merge-drivers.sh, which passes $REGION_FILES "
              "unquoted.", file=sys.stderr)
        return 2
    rc = 0
    for p in paths:
        try:
            lines = open(p, encoding="utf-8", errors="surrogateescape").read().split("\n")
        except OSError as e:
            print(f"merge-docregions --check RED — {p}: {e}", file=sys.stderr)
            rc = 1
            continue
        try:
            regions = scan_regions(lines)
        except Malformed as e:
            print(f"merge-docregions --check RED — {p}: {e}", file=sys.stderr)
            rc = 1
            continue
        if not regions:
            print(f"merge-docregions --check RED — {p}: declares no MERGE-REGION. "
                  "A file carrying `merge=docregions` with no region gets the driver's "
                  "refusals and none of its resolutions, which is strictly worse than "
                  "no attribute at all.", file=sys.stderr)
            rc = 1
            continue
        print(f"merge-docregions --check: {p} -> "
              + ", ".join(f"{r.name}{' (to-eof)' if r.to_eof else ''}"
                          f"{' key=' + r.key if r.key else ''}"
                          f"{' check=' + r.check if r.check else ''}" for r in regions))
    return rc


def main(argv: list[str]) -> int:
    if len(argv) >= 2 and argv[1] == "--check":
        return do_check(argv[2:])
    if len(argv) not in (4, 5):
        print("merge-docregions: usage: BASE OURS THEIRS [PATH]  |  --check FILE...",
              file=sys.stderr)
        return 2
    base, ours, theirs = argv[1], argv[2], argv[3]
    path = argv[4] if len(argv) == 5 else os.path.basename(ours)

    try:
        result, clean, notes = merge_text(base, ours, theirs, path)
    except Malformed as e:
        print(f"merge-docregions: {path}: {e}", file=sys.stderr)
        return 2

    with open(ours, "w", encoding="utf-8", errors="surrogateescape") as f:
        f.write(result)

    for n in notes:
        print("merge-docregions: " + n, file=sys.stderr)
    return 0 if clean else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
