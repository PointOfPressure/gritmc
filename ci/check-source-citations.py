#!/usr/bin/env python3
"""Every `file.rs::symbol` (`:line`) citation, checked against the source.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/check-source-citations.py          verdict on every citation, exit 0/1
#     ci/check-source-citations.py --list   every citation and where it lands
#     ci/check-source-citations.py --fix    rewrite the stale line numbers
#
# WHAT BREAKS IF YOU IGNORE THIS: you cite `power.rs:856` in a roadmap row,
# somebody adds twenty-five lines above it, and the citation now points at a
# comment about thread locals. The row still reads as evidence. Measured
# 2026-08-18: **seven of the twelve** citations in this form were stale, one
# of them by 670 lines.
# ===== end interface; everything below is why =====

# # THE ROW THAT PROMPTED THIS

`docs/PARITY-ROADMAP.md`'s neighbour-update row is stamped **CLOSED** and
carries its evidence as source citations:

    `power.rs::update_neighbors_at` (`:856`) is a faithful
    `CollectingNeighborUpdater` port driven from `shapes.rs::set_block`
    (`:1000`)

Both line numbers are wrong. `update_neighbors_at` is at `:881`; `:856` is a
comment about thread locals. `use_sound` is cited at `:1554` and is at
`:2224`. A reader who follows the citation to check the CLOSED stamp finds
unrelated code and has no way to tell whether the claim moved or the code did.

This is the same failure the brief names for `720`: **an audit that quotes a
figure out of the prose it is auditing has measured the prose.** A line number
pasted into a document is a measurement taken once, at a commit nobody
recorded, and never taken again.

# # WHY THIS NEEDS NO LEDGER, UNLIKE `ci/unported-inventory.py`

`ci/unported-inventory.py` keys its inventory by `file:line` against a named
base commit and carries a fingerprint per row, because the thing it tracks —
an `unported` marker — has no name of its own to search for. It has to
remember what was there.

These citations do not need remembering, because **the document already
states its own oracle**. `` `power.rs::update_neighbors_at` (`:856`) `` names
the symbol *and* the line, so the check is simply whether they agree, and
there is no third artefact to go stale. That is strictly better than a ledger
where it is available: a ledger is a second thing that can drift, and this
project has already been bitten by a stale table twice today.

The corollary is the rule this file enforces: **cite a symbol with the line,
never a line alone.** A bare `world_send.rs:1293` cannot be checked without a
ledger, and there are 96 of those in the tree — they are listed by `--list`
under `UNCHECKABLE` so the count is visible rather than absent, and converting
one to the symbol form is a strict improvement any lane can make in passing.

# # RESOLVING A BAREFOOT FILENAME

`world.rs` names two tracked files (`rustmc-server/src/world.rs` and
`rustmc-worldgen/src/surface/world.rs`). Rather than guess by crate, the
candidate set is narrowed to those that actually **define** the cited symbol;
`replaceable` is defined in exactly one of them, so the citation resolves. If
that still leaves two, the verdict is `AMBIGUOUS` and names both, because
picking one would be the check inventing the answer it is supposed to be
adjudicating.

# # THE DEFECT CLASS THIS MUST NOT JOIN

A citation checker that finds nothing to check passes. So `--selftest`
poisons each verdict on fixtures, and `MIN_CITATIONS` floors how many
citations must resolve at all: deleting the citations, or breaking the regex
so it matches nothing, is red rather than green. On 2026-08-18 the regex
matched 12 and 7 were stale, which is the number that makes the floor
meaningful — a version of this script that silently matched zero would have
been indistinguishable from a clean tree.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Documents that carry citations. Globs rather than a hand-kept list for
# `cold-start/`, which gains a file per branch — the same reasoning as
# `ci/figures.toml`'s `coverage.doc_globs`, and for the same reason: a
# hand-kept list of a directory that grows is a list that goes stale.
DOC_GLOBS = ("docs/*.md", "docs/cold-start/*.md", "ci/checks/*.toml",
             "ci/checks/gameplay/*.toml")

# `` `world_send.rs::use_sound` (`:1554`) `` — symbol and line together. The
# bounded gap allows the handful of live spellings ("… (`:1554`)", "…, at
# (`:193`)") without letting the pattern span a whole paragraph and pair a
# symbol with some unrelated later number.
SYMBOL_CITE = re.compile(r"`([A-Za-z0-9_./-]+\.rs)::([A-Za-z0-9_]+)`([^(\n]{0,40})\(`:(\d+)`\)")

# `` `::run_block_events` (`:389`) `` — the continuation form. The roadmap
# writes a run of members of one file as
#
#     `containers.rs::queue` (`:121`), `::run_block_events` (`:389`),
#     `::block_event_body` (`:446`)
#
# and the first version of this script matched only the first of the three.
# The other two were unchecked and looked checked, which is worse than being
# listed as UNCHECKABLE: three citations were silently outside the census
# while the summary line said every citation had been adjudicated. They happen
# to be correct today. That is luck, and luck is what this file exists to
# remove. The filename is inherited from the nearest `file.rs::` earlier on
# the SAME LINE — never across lines, because a paragraph break is exactly
# where an inherited filename would start naming the wrong file.
CONT_CITE = re.compile(r"`::([A-Za-z0-9_]+)`([^(\n]{0,40})\(`:(\d+)`\)")

# A bare `world_send.rs:1293`. Counted and reported, not adjudicated.
BARE_CITE = re.compile(r"`?([A-Za-z0-9_./-]+\.rs):(\d+)`?")
TOML_KEY_CITE = re.compile(r'^\s*"[A-Za-z0-9_./-]+\.rs:\d+"\s*=')

# `` `power.rs::update_neighbors_at` (was `:856`) `` — a QUOTATION of a
# citation, not a citation.
#
# Found by this checker reddening on the section that documents this checker.
# §59.4 quotes the CLOSED row as it stood — "`power.rs::update_neighbors_at`
# (`:856`) is a faithful port driven from `shapes.rs::set_block` (`:1000`)" —
# because the wrong numbers ARE the evidence. Rewriting them to the repaired
# ones would make the paragraph claim that nothing was ever wrong.
#
# This is the same distinction `ci/scenario-census.py` draws for "4 of 84":
# a quotation of a past claim is not a claim, and a checker that cannot tell
# them apart gets switched off the first time it is right. The census draws it
# by naming the live sites; here that would not work, because the quotation
# and the live citation can sit in the same document and often in the same
# paragraph.
#
# So the DOCUMENT draws it, with a word a reader already understands. `was`
# cannot be applied to a live citation by accident, it reads correctly in
# prose, and the count of them is printed on every run so the exemption cannot
# grow quietly — which is the property that keeps it from being a hole.
HISTORICAL_CITE = re.compile(
    r"`([A-Za-z0-9_./-]+\.rs)::([A-Za-z0-9_]+)`[^(\n]{0,40}\(was `:(\d+)`\)")

# A floor, not a count. See the module docstring: a regex that matches nothing
# passes every rule below it, and that is the whole failure mode this script
# exists to make impossible for line numbers.
MIN_CITATIONS = 8

# THE BARE FORM IS BEING RETIRED, AND THIS IS THE RATCHET THAT RETIRES IT.
#
# A bare `world_send.rs:1293` cannot be adjudicated: nothing in it says what
# was supposed to be there, so a checker can only confirm the line exists. 19
# of the symbol-form citations were stale, one by 670 lines, and a CLOSED row
# cited a line that is now a comment about thread locals — so the bare form is
# not a cosmetic preference. It is the shape in which a CLOSED claim rots
# without anything being able to notice.
#
# Distinct `(file, line)` pairs, not occurrences: quoting one twice is not two
# problems. 96 before `--convert`, 87 after — it does only the case where the
# cited line IS a definition, because that is the only one where the symbol and
# the number mean the same thing.
#
# The other 87 are not laziness. 61 point INTO a function rather than at it, so
# naming the enclosing symbol would assert a relationship the author never
# wrote; 12 name a file whose basename is ambiguous; and a handful are RANGES —
# `speleothem_cluster.rs:252-284`, `shapes.rs:993/1018` — where the symbol form
# has nowhere to put the second number. Each needs a human to say what the
# interesting thing is.
#
# LOWER IT as more are converted; the slack prints on every run. A NEW bare
# citation pushes the count over and reddens, which is what makes the symbol
# form the one the tree accepts rather than the one it prefers.
# RE-BASED 87 -> 97 ON LANDING, 2026-09-04. The 87 was measured on the tree
# this check was written against (2026-08-19). Between that tree and the main
# it lands on (f4047ae1f) ten bare citations were written by other branches —
# `containers.rs:343`, `containers.rs:397`, `entities.rs:19`, `item.rs:32`,
# `ore.rs:521`, `placedby.rs:492`, `record.rs:75`,
# `tools/oracle-runner/src/main.rs:239`, `tools/oracle-runner/src/record.rs:75`,
# `xoroshiro.rs:57` — every one of the kind the paragraph above says a human
# has to name: two cite a doc comment *as* a doc comment, the rest point into
# a function. Naming the enclosing symbol for them would assert what their
# authors never wrote, so the ceiling is the count of the landing tree and the
# direction of the ratchet is unchanged: the eleventh reddens.
# RE-BASED AGAIN 97 -> 100 ON THE STACK, 2026-09-04: four more bare citations
# came in with folded branches written before this check — `power.rs:561`
# (docs/cold-start/refusal-ledger.md), `probe_placement_vs_servers.rs:62`
# (docs/FINDINGS.md), `trace_switch.rs:52` (docs/cold-start/t0-assertion-floor.md)
# and `crates/rustmc-server/src/broadcast.rs:158` (docs/PARITY-ROADMAP.md, a
# doc-comment line) — while `crates/rustmc-data/src/fixture.rs:9` left. Net +3;
# the hundred-and-first reddens. Ledger KEYS of the same shape are no longer
# counted at all (see TOML_KEY_CITE).
# RE-BASED AGAIN 100 -> 101 ON LANDING, 2026-09-05, and for the third time by
# the same cause: `mobs-brains` merged into main under this stack between its
# gate at 4ea71f34b (100 bare, at the ceiling, green) and its rebase onto
# bd5be4a36 (101, red). Measured rather than reasoned about — the bare sets of
# the two trees differ by exactly one entry and nothing left:
#
#   crates/rustmc-entity/src/brain/behavior.rs:178   (docs/FINDINGS.md)
#
# It is of the kind the paragraph above says a human has to name, and
# `--convert` refuses it correctly: line 178 is
# `impl<M: MobHost, B: BehaviorBody<M>> BehaviorControl<M> for Behavior<B> {`,
# an impl header and not a definition, and the symbol its finding is actually
# about — `Behavior.tryStart` — is `fn try_start` at :187, one of THREE in
# that file (:73, :187, :262). `behavior.rs::try_start` would therefore name a
# symbol the checker could not adjudicate to the right one. The lane that owns
# that finding can lower this by one whenever it says which.
#
# NOT raised for anything this stack wrote: it adds no bare citation at all,
# and the count at its own gated tip was exactly 100.
#
# `--convert` was run first and is why this is a ceiling change rather than a
# conversion. It converted the wrong citation and would have been a silent
# corruption: `docs/FINDINGS.md:202` QUOTES `probe_placement_vs_servers.rs:62`
# inside a finding whose whole subject is that a falsifier hardcoded that line
# number and went vacuous when the file moved to `:63`. Rewriting the quoted
# string to the symbol form deletes the sentence's point while leaving it
# reading plausibly. A quotation of a citation is not a citation; see the
# HISTORICAL `(was `:N`)` category, which is the same distinction already
# mechanised for the case where the author marked it.
# RE-BASED 101 -> 103 ON LANDING, 2026-09-24, same cause: main moved
# 3180 -> 3316 under this stack. The bare sets of its green 3180 tip eee81df12
# and of its 3316 rebase were diffed: +3 from main, -1 converted, 0 from this
# stack.
#
#   crates/rustmc-entity/tests/brain_tables.rs:405  (docs/cold-start/mobs-brains-5.md:74)
#   tools/oracle-runner/src/trace.rs:1605           (docs/cold-start/weather.md:136)
#   tools/oracle-runner/src/trace.rs:1701           (docs/cold-start/weather.md:60)
#
# All three name a line INSIDE a function (a panic site in a test's assert,
# and two match arms of `fn decode`, 600 lines long), so the symbol form would
# name the enclosing function and lose the line the sentence is about.
# `world_send.rs:3019` (docs/PARITY-ROADMAP.md) left, `--convert`ed to
# `world_send.rs::water_at`, the definition at that line. `--convert` also
# rewrote the `probe_placement_vs_servers.rs:62` quotation again (see above);
# that was reverted by hand.
# RE-BASED 103 -> 114 ON LANDING, 2026-09-24 (second time that day): main
# moved 3316 -> 3611 (stack-gameplay, 26ffd4965) under this stack, rewriting
# §4's family table and adding four cold starts. Bare sets of this stack's
# green tip 2eedfac17 and of its 3611 rebase diffed after --fix and --convert:
# +14, -3, every added citation present in origin/main's own text —
# attack.rs:66, physics.rs:182, physics.rs:205, power.rs:786, status.rs:497,
# tracker.rs:652, useblock.rs:102, world_send.rs:1009, world_send.rs:2552
# (PARITY-ROADMAP), power.rs:2427 and placement.rs:971
# (cold-start/gameplay-block-schemas.md), power.rs:768
# (checks/server-level-gates.toml), compare.rs:156 (cold-start/gate-instruments.md),
# and world_send.rs:2552 again under its full path (cold-start/gameplay-sound-coverage.md).
# Three left (a compare.rs, a placedby.rs and a power.rs line). --convert's rewrite of
# the probe_placement_vs_servers.rs:62 quotation was reverted again.
# RE-BASED 114 -> 117 ON train-c, 2026-09-30: main moved 3611 -> 3745
# (c52d52e7b) under this stack. 114 was already one short of the tree it was
# set on — e3ef75850 measures 115 (its gate, 2026-09-28 23:11, went RED on
# exactly that); the uncounted one was `client.rs:1091`, which has since left.
# Bare sets of the stack tip e03efe1f8 and of its rebase onto c52d52e7b,
# diffed after --fix and --convert: +3, -1, every added citation in
# origin/main's own docs/FINDINGS.md, which is append-only and whose lines
# quote a line number as it stood when the finding was written:
#
#   block_props.rs:75                             (a doc comment, quoted)
#   crates/rustmc-behavior/src/block_props.rs:499 (inside a function)
#   sky.rs:1205                                   (an assert line in a test)
#
# Converting any of them would rewrite another lane's finding to name a
# symbol it did not cite. --convert's rewrite of the
# probe_placement_vs_servers.rs:62 quotation was reverted a third time.
# 117 -> 119 ON train-c WITH light-engine-2 (2026-09-30): two distinct bare
# forms of one citation, `tools/oracle-runner/src/trace.rs:247` (FINDINGS.md,
# cold-start/light-engine.md) and `trace.rs:247` (WORKLIST.md's index row).
# All three quote the line where LIGHT_UPDATE sat in the housekeeping set on
# 2026-09-05 — the state this very branch changed — so the symbol form would
# re-point a finding about the old filter at the new one. Quotations, not
# citations; FINDINGS.md is append-only.
# 119 -> 120 ON train-c WITH chunk-streaming (2026-09-30): `broadcast.rs:114`
# (docs/FINDINGS.md, append-only), which points into the doc comment above
# `broadcast.rs`'s view-distance note — a doc paragraph cited as a doc
# paragraph, the kind the RE-BASED 97 block says a human must name.
# 120 -> 125 ON train-c's rebase onto main 3953 (train-a, 2026-09-30): five
# bare citations, every one in main's own docs (docs/FINDINGS.md, append-only,
# and docs/cold-start/world-systems.md) — `attributes.rs:44`, `compare.rs:139`,
# `containers.rs:395`, `golden_noise.rs:5`, `golden_rng.rs:135`. Not raised for
# anything train-c wrote.
# 125 -> 174 ON train-g (main 16529d006 + item-components and eight lanes,
# 2026-10-01): forty-nine distinct bare citations, every one in the lanes' own
# docs as they were pushed — docs/FINDINGS.md (append-only), docs/PARITY-
# ROADMAP.md, docs/WORKLIST.md's index rows and the lanes' cold starts — and
# none in anything train-g wrote (its FINDINGS lines and cold start cite
# symbols or files, never `file.rs:line`). 29 stale SYMBOL citations were
# re-pointed in the same commit (`--fix`, and three ambiguous ones by hand).
MAX_BARE_CITATIONS = 174


def strip_elision(name: str) -> str:
    """`.../golden_surface_chunk.rs` -> `golden_surface_chunk.rs`.

    `ci/checks/test-targets.toml` shortens a long path with a leading `...`
    where the surrounding prose has already given the directory. Reporting
    that as a missing file would be a false red on a citation a reader can
    follow perfectly well.
    """
    return name[4:] if name.startswith(".../") else name


def tracked_rs() -> dict[str, list[str]]:
    # `git ls-files` is the tracked set; the merge gate runs T0 in an rsynced
    # directory that is not a git repository (the same fact behind the SKIPPED
    # lines `check-cited-audits` and `figure-provenance` print), and there the
    # first gate of the claims-allocator stack died on exit 128 here, 2026-09-04,
    # before scanning a single citation. Walking the shipped tree is the same
    # set: the rsync excludes `target`, `.git` and `oracle-work`, and nothing
    # under `crates`, `tools` or `plugins` is an untracked `.rs`.
    proc = subprocess.run(
        ["git", "-C", str(ROOT), "ls-files", "*.rs"],
        capture_output=True, text=True,
    )
    if proc.returncode == 0:
        out = proc.stdout.split()
    elif (ROOT / ".gitstore").exists() and subprocess.run(
            ["git", "--git-dir", str(ROOT / ".gitstore"), "ls-tree", "-r",
             "--name-only", "HEAD"], capture_output=True, text=True).returncode == 0:
        # The gate copy ships a bare object store with HEAD at the gated commit
        # (ci/t0-remote.sh, ci/lib/box_env.py); its tree listing IS the tracked set.
        out = [p_ for p_ in subprocess.run(
            ["git", "--git-dir", str(ROOT / ".gitstore"), "ls-tree", "-r",
             "--name-only", "HEAD"], capture_output=True, text=True).stdout.split()
            if p_.endswith(".rs")]
    else:
        out = sorted(
            str(q.relative_to(ROOT))
            for top in ("crates", "tools", "plugins")
            for q in (ROOT / top).rglob("*.rs")
            if "target" not in q.relative_to(ROOT).parts
        )
    by_name: dict[str, list[str]] = {}
    for p in out:
        by_name.setdefault(Path(p).name, []).append(p)
        by_name.setdefault(p, []).append(p)
    return by_name


def defines(path: Path, symbol: str) -> list[int]:
    """1-based lines defining `symbol` as a fn, struct, enum, const or static.

    Deliberately broad: the citations in this tree name functions almost
    always but `containers.rs::queue` is a field and `sounds.rs::lever_click`
    is a const, and a checker that only understood `fn` would report those as
    stale when they are correct — a false red is how a check gets switched
    off.
    """
    pat = re.compile(
        rf"^\s*(?:pub(?:\([^)]*\))?\s+)?"
        rf"(?:async\s+|unsafe\s+|const\s+|static\s+)*"
        rf"(?:fn|struct|enum|trait|type|const|static)\s+{re.escape(symbol)}\b"
    )
    hits = []
    for i, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
        if pat.match(line):
            hits.append(i)
    return hits


def resolve(name: str, symbol: str, index: dict[str, list[str]]) -> tuple[list[str], list[str]]:
    """(candidates, narrowed). See the module docstring on barefoot names."""
    cands = sorted(set(index.get(strip_elision(name), [])))
    if len(cands) <= 1:
        return cands, cands
    narrowed = [c for c in cands if defines(ROOT / c, symbol)]
    return cands, narrowed


def scan() -> tuple[list[dict], list[dict], list[dict]]:
    """(adjudicated citations, bare citations, historical quotations)."""
    index = tracked_rs()
    sym_cites: list[dict] = []
    bare: list[dict] = []
    historical: list[dict] = []
    files: list[Path] = []
    for g in DOC_GLOBS:
        files.extend(sorted(ROOT.glob(g)))
    for doc in files:
        try:
            lines = doc.read_text(errors="replace").splitlines()
        except OSError:
            continue
        for i, line in enumerate(lines, 1):
            claimed_spans = []
            # (start, filename) for every qualified citation on this line, so a
            # continuation can inherit the filename that precedes it.
            # A historical citation is claimed FIRST, so the live patterns
            # cannot also match inside it.
            hist_spans = [m.span() for m in HISTORICAL_CITE.finditer(line)]
            historical.extend(
                {"doc": doc.relative_to(ROOT).as_posix(), "line": i,
                 "name": m.group(1), "symbol": m.group(2), "cited": int(m.group(3))}
                for m in HISTORICAL_CITE.finditer(line)
            )
            anchors = [(m.start(), m.group(1)) for m in SYMBOL_CITE.finditer(line)]
            found = [(m, m.group(1), m.group(2), int(m.group(4)))
                     for m in SYMBOL_CITE.finditer(line)]
            for m in CONT_CITE.finditer(line):
                prior = [n for pos, n in anchors if pos < m.start()]
                if not prior:
                    continue  # a `::sym` with no file before it names nothing
                found.append((m, prior[-1], m.group(1), int(m.group(3))))
            found = [t for t in found
                     if not any(a <= t[0].start() < b for a, b in hist_spans)]
            found.sort(key=lambda t: t[0].start())
            for m, name, symbol, num in found:
                claimed_spans.append(m.span())
                cands, narrowed = resolve(name, symbol, index)
                rec = {
                    "doc": doc.relative_to(ROOT).as_posix(), "line": i,
                    "name": name, "symbol": symbol, "cited": num,
                    "match": m.group(0),
                }
                if not cands:
                    rec["verdict"] = "UNRESOLVED"
                    rec["detail"] = f"no tracked file named {name}"
                elif len(narrowed) != 1:
                    rec["verdict"] = "AMBIGUOUS"
                    rec["detail"] = (
                        f"{name} names {len(cands)} tracked files and "
                        f"{len(narrowed)} of them define `{symbol}`: "
                        + ", ".join(narrowed or cands)
                        + " — qualify the citation with a path"
                    )
                else:
                    src = ROOT / narrowed[0]
                    body = src.read_text(errors="replace").splitlines()
                    rec["path"] = narrowed[0]
                    rec["defs"] = defines(src, symbol)
                    at = body[num - 1] if 0 < num <= len(body) else None
                    if at is None:
                        rec["verdict"] = "PAST-EOF"
                        rec["detail"] = f"{narrowed[0]} has {len(body)} lines"
                    elif symbol in at:
                        rec["verdict"] = "OK"
                        rec["detail"] = at.strip()[:70]
                    else:
                        rec["verdict"] = "STALE"
                        rec["detail"] = (
                            f"line {num} is {at.strip()[:56]!r}; "
                            f"`{symbol}` is defined at "
                            + (", ".join(f":{d}" for d in rec["defs"]) or "no line in this file")
                        )
                sym_cites.append(rec)
            # A TOML ledger KEY is not a citation. `ci/checks/comment-guards.toml`
            # and `ci/checks/shell-footguns.toml` are ratchets keyed by
            # `"file.rs:line" = "CLASS"`; the key is the instance's identity,
            # its own checker refuses it when the line moves, and the symbol
            # form has nowhere to go in a key. Counting them here made every
            # declared instance a bare citation (2026-09-04, two of them).
            if TOML_KEY_CITE.match(line):
                continue
            for m in BARE_CITE.finditer(line):
                if any(s <= m.start() < e for s, e in claimed_spans):
                    continue
                if any(a <= m.start() < b for a, b in hist_spans):
                    continue
                bare.append({
                    "doc": doc.relative_to(ROOT).as_posix(), "line": i,
                    "name": m.group(1), "cited": int(m.group(2)),
                })
    return sym_cites, bare, historical


BAD = ("STALE", "PAST-EOF", "AMBIGUOUS", "UNRESOLVED")


DEF_LINE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+|unsafe\s+|const\s+|static\s+)*"
    r"(?:fn|struct|enum|trait|type|const|static)\s+([A-Za-z0-9_]+)\b")


def convert(bare) -> int:
    """Rewrite bare citations whose line IS a definition into the symbol form.

    ONLY that case. A bare citation pointing INTO a function could be given
    the enclosing function's name, and 61 of the 96 are that shape — but the
    line number then means something different from the symbol beside it, and
    the checker would start asserting a relationship the author never wrote.
    Those need a human to say whether the interesting thing is the definition
    or the line, so they are listed and left.
    """
    index = tracked_rs()
    edits: dict[str, list[tuple[str, str]]] = {}
    done: set[tuple[str, int]] = set()
    left: list[dict] = []
    for b in bare:
        key = (b["name"], b["cited"])
        cands = sorted(set(index.get(strip_elision(b["name"]), [])))
        if len(cands) != 1:
            left.append(dict(b, why=f"{len(cands)} candidate files"))
            continue
        body = (ROOT / cands[0]).read_text(errors="replace").splitlines()
        n = b["cited"]
        if not (0 < n <= len(body)):
            left.append(dict(b, why="past EOF"))
            continue
        m = DEF_LINE.match(body[n - 1])
        if not m:
            left.append(dict(b, why="line is not a definition"))
            continue
        # Keyed by (doc, name, line), not (name, line). Keying globally meant
        # the FIRST document carrying a citation got rewritten and every other
        # one was skipped as a duplicate — `ids.rs:193` was converted in
        # PARITY-ROADMAP and left bare in a cold start, which is worse than
        # not converting it, because the two documents then disagree.
        if (b["doc"], *key) in done:
            continue
        done.add((b["doc"], *key))
        old_txt = f"`{b['name']}:{n}`"
        new_txt = f"`{b['name']}::{m.group(1)}` (`:{n}`)"
        edits.setdefault(b["doc"], []).append((old_txt, new_txt, b, m.group(1)))
    # Apply, and count only what actually moved.
    #
    # The first version printed a conversion for every candidate and wrote
    # whatever `str.replace` happened to match, which is not the same thing:
    # `speleothem_cluster.rs:252-284` is matched by the scanner as `:252` but
    # the literal `` `...rs:252` `` — with the closing backtick — does not
    # occur, so two DETERMINISM.md conversions were REPORTED AND NOT MADE. A
    # tool that reports work it did not do is worse than one that does
    # nothing, because the next person believes it.
    applied = 0
    for rel, subs in edits.items():
        path = ROOT / rel
        text = path.read_text()
        for a, bb, b, sym in subs:
            if a not in text:
                left.append(dict(b, why="not a bare literal — a range or a "
                                        "compound like `f.rs:12-40` or `f.rs:12/18`"))
                continue
            text = text.replace(a, bb)
            applied += 1
            print(f"{b['doc']}:{b['line']}  {b['name']}:{b['cited']} -> ::{sym}")
        path.write_text(text)
    n_left = len({(x["name"], x["cited"]) for x in left})
    print(f"\nconverted {applied} occurrence(s); {n_left} distinct left for a human")
    print("A RANGE is not a line and is left alone: `speleothem_cluster.rs:252-284` "
          "and `shapes.rs:993/1018` name a span and a pair, and the symbol form "
          "has nowhere to put the second number.")
    by_why: dict[str, int] = {}
    for x in left:
        by_why[x["why"]] = by_why.get(x["why"], 0) + 1
    for why, k in sorted(by_why.items()):
        print(f"  {k}: {why}")
    return 0


def report(cites, bare, historical, show_all: bool) -> int:
    reds = [c for c in cites if c["verdict"] in BAD]
    for c in cites:
        if show_all or c["verdict"] in BAD:
            print(f"{c['verdict']:<10} {c['doc']}:{c['line']}  "
                  f"{c['name']}::{c['symbol']} cited :{c['cited']}")
            print(f"           {c['detail']}")
    if show_all and bare:
        print(f"\nUNCHECKABLE {len(bare)} bare `file.rs:line` citations carry no "
              "symbol, so nothing can adjudicate them. Converting one to the "
              "`file.rs::symbol` (`:line`) form makes it checkable:")
        for b in sorted({(b["name"], b["cited"], b["doc"]) for b in bare}):
            print(f"           {b[2]} -> {b[0]}:{b[1]}")

    if historical:
        # Always printed, never hidden behind --list: an exemption whose size
        # is invisible is an exemption that grows.
        print(f"\nHISTORICAL {len(historical)} citation(s) written `(was `:N`)` "
              "— quotations of a past citation, deliberately not adjudicated:")
        for h in historical:
            print(f"           {h['doc']}:{h['line']} {h['name']}::{h['symbol']} was :{h['cited']}")

    print(f"\nsource-citations: {len(cites)} symbol citations, "
          f"{len(cites) - len(reds)} land on their symbol, {len(reds)} do not; "
          f"{len(historical)} are historical quotations; "
          f"{len(bare)} bare citations are uncheckable")

    distinct_bare = len({(b["name"], b["cited"]) for b in bare})
    print(f"source-citations: {distinct_bare} distinct bare citations of "
          f"ceiling {MAX_BARE_CITATIONS}")
    if distinct_bare > MAX_BARE_CITATIONS:
        print(f"\nRED: {distinct_bare} distinct bare `file.rs:line` citations, "
              f"ceiling is {MAX_BARE_CITATIONS}. A bare citation says nothing "
              "about what was supposed to be at that line, so nothing can tell "
              "you when it stops being true — which is how 19 symbol citations "
              "went stale unnoticed. Write `file.rs::symbol` (`:line`); "
              "`--convert` does the ones whose line is a definition.")
        return 1

    if len(cites) < MIN_CITATIONS:
        print(f"\nRED: only {len(cites)} citations matched, floor is "
              f"{MIN_CITATIONS}. A pattern that matches nothing passes every "
              "rule above it — see MIN_CITATIONS.")
        return 1
    if reds:
        print("\nRED: a citation that does not land on its symbol is evidence "
              "pointing at unrelated code. `--fix` rewrites the STALE ones.")
        return 1
    return 0


def fix(cites) -> int:
    """Rewrite STALE line numbers where exactly one definition exists.

    Only STALE, and only with one candidate definition: AMBIGUOUS and
    UNRESOLVED need a human to say which file is meant, and a `--fix` that
    guessed would turn a loud wrong citation into a quiet one.
    """
    edits: dict[str, list[dict]] = {}
    for c in cites:
        if c["verdict"] == "STALE" and len(c.get("defs", [])) == 1:
            edits.setdefault(c["doc"], []).append(c)
    n = 0
    for doc, cs in sorted(edits.items()):
        p = ROOT / doc
        text = p.read_text()
        for c in cs:
            old = c["match"]
            new = old.replace(f"(`:{c['cited']}`)", f"(`:{c['defs'][0]}`)")
            if old in text:
                text = text.replace(old, new)
                n += 1
                print(f"fixed {doc}:{c['line']}  {c['name']}::{c['symbol']} "
                      f":{c['cited']} -> :{c['defs'][0]}")
        p.write_text(text)
    skipped = [c for c in cites if c["verdict"] in BAD and c not in
               [x for v in edits.values() for x in v]]
    if skipped:
        print(f"\n{len(skipped)} left for a human — ambiguous, unresolved, or "
              "with more than one definition of the symbol:")
        for c in skipped:
            print(f"  {c['doc']}:{c['line']} {c['name']}::{c['symbol']} — {c['verdict']}")
    print(f"\nfixed {n} citation(s)")
    return 0


# ---------------------------------------------------------------------------


def _selftest() -> int:
    import tempfile
    ok = True

    def expect(label, cond):
        nonlocal ok
        if not cond:
            print(f"SELFTEST FAIL [{label}]", file=sys.stderr); ok = False
        else:
            print(f"SELFTEST ok   [{label}]")

    with tempfile.TemporaryDirectory() as td:
        d = Path(td)
        src = d / "x.rs"
        src.write_text("// a\n// b\npub fn thing() {}\n// d\nconst LEVER: u8 = 1;\n")
        expect("a fn definition is found at its line", defines(src, "thing") == [3])
        expect("a const definition is found too — not only fns",
               defines(src, "LEVER") == [5])
        expect("a symbol that is not defined finds nothing",
               defines(src, "absent") == [])
        expect("a mention that is not a definition is not a definition",
               defines(src, "a") == [])

    # The regex is the part most likely to silently stop matching, so it is
    # asserted against the exact spellings live in the tree today.
    live = [
        "`power.rs::update_neighbors_at` (`:856`)",
        "`shapes.rs::set_block` (`:1000`)",
        "`sounds.rs::lever_click` (`:193`)",
        "driven from `world.rs::replaceable` (`:867`), which is",
    ]
    hits = [SYMBOL_CITE.search(s) for s in live]
    expect("every live citation spelling still matches", all(hits))
    expect("the match captures file, symbol and line",
           hits[0].group(1) == "power.rs" and hits[0].group(2) == "update_neighbors_at"
           and hits[0].group(4) == "856")
    expect("a bare file:line is NOT taken for a symbol citation",
           SYMBOL_CITE.search("`world_send.rs:1293`") is None)
    expect("a symbol far from its number is not paired with it",
           SYMBOL_CITE.search("`a.rs::f` " + "x" * 60 + " (`:1`)") is None)
    expect("a bare citation IS matched by the bare pattern",
           BARE_CITE.search("`world_send.rs:1293`").group(2) == "1293")

    run = ("`containers.rs::queue` (`:121`), `::run_block_events` (`:389`), "
           "`::block_event_body` (`:446`)")
    expect("the continuation form is matched — two of these three used to be invisible",
           len(CONT_CITE.findall(run)) == 2)
    expect("and it captures the symbol and the line",
           CONT_CITE.search(run).group(1) == "run_block_events"
           and CONT_CITE.search(run).group(3) == "389")
    expect("a `::sym` with no filename before it on the line is not guessed at",
           SYMBOL_CITE.search("`::orphan` (`:1`)") is None)

    hist = "`power.rs::update_neighbors_at` (was `:856`)"
    expect("a `(was `:N`)` quotation is recognised as historical",
           HISTORICAL_CITE.search(hist) is not None)
    expect("and it captures what was cited",
           HISTORICAL_CITE.search(hist).group(3) == "856")
    expect("a LIVE citation is not mistaken for a historical one",
           HISTORICAL_CITE.search("`power.rs::update_neighbors_at` (`:881`)") is None)
    expect("and the historical form is not also matched as a live citation "
           "by the naive pattern order",
           SYMBOL_CITE.search(hist) is None)

    print("SELFTEST " + ("PASS" if ok else "FAIL"))
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--fix", action="store_true")
    ap.add_argument("--convert", action="store_true")
    ap.add_argument("--selftest", action="store_true")
    a = ap.parse_args()
    if a.selftest:
        return _selftest()
    cites, bare, historical = scan()
    if a.fix:
        return fix(cites)
    if a.convert:
        return convert(bare)
    return report(cites, bare, historical, show_all=a.list)


if __name__ == "__main__":
    sys.exit(main())
