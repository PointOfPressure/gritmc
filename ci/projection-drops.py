#!/usr/bin/env python3
"""Enumerate every field the corpus projection can never compare, and refuse a new one.

# What this measures, and why `rest` is the whole distinction

`tools/oracle-runner/src/trace.rs::observe` digests **the bytes the decoder did
not consume** into a field called `rest`, and `rest` is compared exactly. So a
wire field has one of two fates and they are not close together:

* **left unread** — it stays in `rest` and is still compared, byte for byte,
  without this harness understanding it. Opaque, but *live*.
* **read and not emitted** — advancing the reader is precisely what removes
  those bytes from `rest`. Nothing downstream can ever see them again. The
  field is not merely unnamed, it is **invisible**, and no check can fail on it.

The second class is the subject of this script. It has been rediscovered three
times by accident and never enumerated: `sound.pitch`, which decided claim 420
and was thrown away; the item-stack fields that made 607's greens uncitable
before 13 Aug; and the entity ids in `damage_event`, whose trap was armed and
happened not to be sprung.

There is a second stage. `compare::project` deletes whole observations and
individual fields *after* decoding, and `compare::informative` subtracts
observations from the count that decides whether a check is vacuous. A field
removed there is equally uncomparable, so those sites are enumerated too.

# The mechanism: an annotation, not a heuristic

A detector that tried to judge which discards are legitimate would be a
detector nobody could trust. Instead every discard site must carry a marker
comment on one of the lines above it:

    // DROP: <name> — <why>

and `<name>` must appear in `ci/checks/projection-drops.toml` with its impact
assessed. Five things are then refusable and each is a real defect:

1. a discard with **no marker** — a field silently removed from comparison;
2. a marker whose name is **not in the ledger** — a drop nobody ranked;
3. a ledger row with **no site** — a stale entry claiming coverage that moved;
4. a ledger row whose `observed_by` no longer matches the checks that actually
   name that kind — the ranking gone stale under a check-file edit;
5. a ledger row whose `stage` disagrees with the site carrying its marker — the
   drop filed at the wrong layer, which misreports what it costs to undo.

(4) is what makes this more than a list. A drop is a note when no check reads
the kind it belongs to and a defect when one does, and that is a property of
`ci/checks/gameplay/*.toml`, which change far more often than the decoder.

(5) is the one that had already happened. `add_entity.movement` moved from
`trace::decode` to `compare::drop_add_entity_movement` on 2026-08-18 and its row
went on saying `decode` across a branch boundary, while `--list` printed
`project` for it in the same run `--check` passed it. See `STAGES` below.

Run with no arguments to check; `--list` to print the sites; `--rank` to print
the sites joined to the checks that observe them.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_files as cf  # noqa: E402 — must follow the sys.path line
import trace_source as ts  # noqa: E402 — same

# `PROJECTION_DROPS_ROOT` points the reader at a copy of the tree. It exists
# for `ci/projection-drops-falsify.sh`, which has to mutate the decoder and the
# ledger to show this check refusing them, and must not do that in the tree it
# is checking. Unset — which is every real run — it is the tree this file is in.
ROOT = os.environ.get("PROJECTION_DROPS_ROOT") or os.path.dirname(
    os.path.dirname(os.path.abspath(__file__))
)
TRACE = os.path.join(ROOT, "tools/oracle-runner/src/trace.rs")
COMPARE = os.path.join(ROOT, "tools/oracle-runner/src/compare.rs")
LEDGER = os.path.join(ROOT, "ci/checks/projection-drops.toml")

MARKER = re.compile(r"//\s*DROP:\s*([A-Za-z0-9_.]+)\s*(?:[-—]\s*(.*))?$")

# See the ledger's own header for what separates these. The axis is what the
# dropped field could ever decide, not whether some check happens to name its
# kind — eleven checks observe `add_entity` and none could want its uuid.
IMPACTS = ("decides", "armed", "note")

# The three places a field can be removed, and the ledger's `stage` names one.
#
# **This is not a label, it is the recoverability of the field**, which is why
# it is now checked against the site rather than trusted. A field removed at
# `decode` is gone from `rest` before anything downstream exists: recovering it
# means changing the recorder, and until somebody does, no instrument here can
# so much as ask a question about it. A field removed at `project` or
# `informative` was recorded and is thrown away by the comparator, so it is
# recoverable by editing a projection — and, more to the point, it is *visible*
# to `ci/trace_vocabulary.py`, `ci/determinism-gap.py` and
# `ci/hidden-stream-audit.py` in the meantime.
#
# `add_entity.movement` is why this check exists. The discard moved from
# `trace::decode` to `compare::drop_add_entity_movement` on 2026-08-18 and the
# ledger row went on saying `decode` for a day, across a branch boundary,
# while `--list` printed `project` for the same row. **Every fact needed to
# catch it was already in this file and no line compared the two.** A ledger
# whose whole purpose is ranking how badly a drop hurts had its recoverability
# column carried as a memory.
STAGES = ("decode", "project", "informative")

# A read whose value is bound to a discard, or not bound at all. Both remove
# the bytes from `rest`; only the spelling differs.
DISCARD_PATTERNS = [
    # `let _seed = read_i64(&mut r).ok()?;`
    (re.compile(r"^\s*let\s+(_[A-Za-z0-9_]*)\s*="), "let-underscore"),
    # `read_u8(&mut r).ok()?;` — a read in statement position, value dropped.
    (re.compile(r"^\s*read_[a-z0-9_]+\(&mut r\)[^;]*;\s*$"), "bare-read"),
    # `read_lp_vec3(&mut r)?;` — the same, through a helper.
    (re.compile(r"^\s*read_[a-z0-9_]+\(&mut r\)\?;\s*$"), "bare-read"),
    # `std::io::Read::read_exact(&mut r, &mut sig).ok()?;` with no later use.
    (re.compile(r"^\s*std::io::Read::read_exact\(&mut r,"), "read-exact"),
]

# `for _ in 0..16 { read_u8(&mut r).ok()?; }` — a counted bulk skip.
BULK = re.compile(r"^\s*for\s+_\s+in\s+0\.\.(\d+)\s*\{")

# Second-stage removals, in compare.rs.
COMPARE_PATTERNS = [
    (re.compile(r"^\s*out\.remove\(\"([A-Za-z0-9_]+)\"\)"), "project-remove-field"),
    (
        re.compile(r"^\s*\.filter\(\|\(o, _\)\| o\.kind != crate::trace::([A-Z_]+)"),
        "project-drop-kind",
    ),
    # The one table-driven filter `recorder-housekeeping` put in place of the
    # per-kind lines above (2026-09-24). Without this pattern the scan did not
    # see it at all: the whole ambient drop would have been a discard with no
    # site, which is refusal 1's defect in the refusal's own scanner.
    (
        re.compile(r"^\s*\.filter\(\|\(o, _\)\| !crate::trace::(is_ambient_kind)\("),
        "project-drop-kind",
    ),
    (
        re.compile(r"^\s*if kind == \"([a-z0-9_]+)\"\s*\{\s*$"),
        "informative-subtract-kind",
    ),
    # `informative` also subtracts **one `block_update` per (step, position)
    # the step was always going to produce**, which is a conditional
    # subtraction rather than a whole kind and is the one that decided 420.
    (
        re.compile(r"^\s*(slot)\.remove\(i\);\s*$"),
        "informative-subtract-allowance",
    ),
]


def read(path):
    with open(path, encoding="utf-8") as fh:
        return fh.read().splitlines()


def marker_above(lines, i):
    """The `DROP:` marker attached to line `i`, searching the comment block above it."""
    j = i - 1
    while j >= 0:
        s = lines[j].strip()
        if not s.startswith("//"):
            break
        m = MARKER.search(s)
        if m:
            return m.group(1), (m.group(2) or "").strip()
        j -= 1
    return None, None


def is_comment(line):
    return line.strip().startswith("//")


def scan_trace():
    """Every site in `decode` that advances the reader without emitting a field."""
    lines = read(TRACE)
    try:
        start = next(i for i, l in enumerate(lines) if l.startswith("fn decode("))
    except StopIteration:
        sys.exit("projection-drops: fn decode not found in trace.rs")
    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i].startswith("#[cfg(test)]")),
        len(lines),
    )

    arm = "<none>"
    sites = []
    skip_until = -1
    for i in range(start, end):
        line = lines[i]
        if is_comment(line):
            continue
        # Inside a counted bulk skip the reads are the skip, not separate
        # discards. Counting them twice would put a second unmarkable site on
        # the same bytes and make the ledger impossible to satisfy.
        if i < skip_until:
            continue
        g = re.match(r"\s*x if x == cb::([A-Z_0-9]+)", line)
        if g:
            arm = g.group(1)
            # `x if x == cb::A || x == cb::B =>` — record both, joined.
            extra = re.findall(r"x == cb::([A-Z_0-9]+)", line)
            if len(extra) > 1:
                arm = "/".join(extra)
            continue

        kindpat = None
        for pat, what in DISCARD_PATTERNS:
            if pat.match(line):
                kindpat = what
                break
        if kindpat == "read-exact":
            # `read_exact` fills a buffer, and a buffer that is later emitted
            # is not a discard at all. `player_chat` does both in one arm: the
            # sender uuid is read into `uuid` and inserted, the 256-byte
            # signature is read into `sig` and never mentioned again.
            buf = re.search(r"&mut ([A-Za-z0-9_]+)\)", line)
            arm_end = next(
                (j for j in range(i + 1, end) if re.match(r"\s*x if x == cb::", lines[j])),
                end,
            )
            if buf and any(
                buf.group(1) in lines[j] and not is_comment(lines[j])
                for j in range(i + 1, arm_end)
            ):
                kindpat = None
        b = BULK.match(line)
        if b and not kindpat:
            # Only a bulk skip if the body does nothing but read.
            body, j = [], i + 1
            while j < end and "}" not in lines[j]:
                if not is_comment(lines[j]) and lines[j].strip():
                    body.append(lines[j].strip())
                j += 1
            if body and all(re.match(r"read_[a-z0-9_]+\(&mut r\)", s) for s in body):
                kindpat = "bulk-skip"
                skip_until = j
        if not kindpat:
            continue

        name, why = marker_above(lines, i)
        sites.append(
            {
                "stage": "decode",
                "file": "trace.rs",
                "arm": arm,
                "how": kindpat,
                "line": i + 1,
                "name": name,
                "why": why,
                "src": line.strip(),
            }
        )
    return sites


def scan_compare():
    lines = read(COMPARE)
    sites = []
    for i, line in enumerate(lines):
        if is_comment(line):
            continue
        for pat, what in COMPARE_PATTERNS:
            m = pat.match(line)
            if not m:
                continue
            name, why = marker_above(lines, i)
            sites.append(
                {
                    "stage": what.split("-")[0],
                    "file": "compare.rs",
                    "arm": "block_update" if what.endswith("allowance") else m.group(1),
                    "how": what,
                    "line": i + 1,
                    "name": name,
                    "why": why,
                    "src": line.strip(),
                }
            )
    return sites


def load_ledger():
    """The ledger, as {name: {key: value}}.

    Parsed with `tomllib`, not by hand. The hand-rolled version this replaced
    read one `key = value` per line and pulled arrays out with a regex, which
    works for the file as written and breaks the moment somebody wraps a long
    `observed_by` across lines — and then the row is read with half its
    observers and this check goes **red on a correctly-edited ledger**. A
    false red on a T0 gate is worse than the parser being longer: it blocks
    the merge train and teaches the next reader that the check is unreliable.

    Values are normalised to `str` and `list[str]` so the callers below can
    stay as they were; `observed_by_count` is compared as a string of digits
    on purpose, because `isdigit()` is also how a missing or misspelt value is
    caught.
    """
    if not os.path.exists(LEDGER):
        return {}
    import tomllib

    with open(LEDGER, "rb") as fh:
        doc = tomllib.load(fh)
    rows = {}
    for entry in doc.get("drop", []):
        row = {}
        for key, val in entry.items():
            row[key] = val if isinstance(val, list) else str(val)
        if "name" in row:
            rows[row["name"]] = row
    return rows


def emittable_kinds():
    """Every `kind` string `trace::observe` can ever produce.

    Three sources and the third is the one with a hole in it:

    * the arm-tail literals in `decode` — `"block_update".into()` and its
      thirty-four siblings, matched at end of line so that an `f.insert` key
      two lines above cannot be mistaken for one;
    * `HEARTBEAT`, `ENTITY_POSITION` and `CHUNK_BLOCK_ENTITIES`, which
      `decode` returns through constants rather than literals (the third
      arrived with main's block-entity arm and read as "no arm emits it"
      until it was listed here, 2026-09-23);
    * `packet:0xNN` for any id with no arm — **except** the twelve
      `is_housekeeping` refuses, which return `None` from `observe` before
      `decode` is reached and can therefore never appear under any name.

    A check that names a kind outside this set observes nothing, in every run,
    for ever. It is not a check that is hard to fail; it is one whose subject
    the recorder removed before either side was looked at.
    """
    text = "\n".join(read(TRACE))
    body = text[text.index("fn decode(") :]
    body = body[: body.index("#[cfg(test)]")] if "#[cfg(test)]" in body else body
    kinds = set(re.findall(r'^\s+"([a-z_0-9]+)"\.into\(\)$', body, re.M))
    for const in re.findall(
        r'pub const (?:HEARTBEAT|ENTITY_POSITION|CHUNK_BLOCK_ENTITIES|CHUNK_NO_BLOCK_ENTITIES): &str = "([a-z_0-9]+)"',
        text,
    ):
        kinds.add(const)
    return kinds


def housekeeping_ids():
    """The `cb::` constant names `trace::observe` drops at record time, and their ids."""
    text = "\n".join(read(TRACE))
    # **`return {}` used to live here** and it is the same defect the id table
    # below was fixed for: a scrape that stops matching hands back an empty set,
    # every `packet:0xNN` looks unresolvable, and the check reports
    # `0 kinds never recorded` having tested nothing. The refusal is now
    # `ci/trace_source.py`'s and it is shared with the two other scripts that
    # ask this question.
    try:
        names = set(ts.housekeeping_names(text))
    except ts.TraceScrapeError as e:
        sys.exit(f"projection-drops: {e}")
    ids_rs = os.path.join(ROOT, "crates/rustmc-proto/src/ids.rs")
    out = {}
    # **A missing id table is a refusal, not an empty answer.** It was the
    # second, and it cost the falsifier's `housekeeping-kind` case: with no
    # `ids.rs` the table came back empty, every `packet:0xNN` looked
    # unresolvable and was let through, and the check printed
    # `0 kinds never recorded` and exited 0 having tested nothing. That is
    # precisely the shape of defect this whole file exists to refuse, so it
    # would not do to have one here.
    if not os.path.exists(ids_rs):
        sys.exit(f"projection-drops: cannot read {ids_rs}, so no packet id can be resolved")
    if True:
        # The `play::cb` block only — the same constant names exist under
        # other states with other values, and `KEEP_ALIVE` has three of them.
        # Taking the *last* `pub mod cb` picks up `configuration::cb`, which is
        # a different protocol state and holds none of these ids.
        src = open(ids_rs, encoding="utf-8").read()
        play = src.index("pub mod play {")
        start = src.index("pub mod cb {", play)
        # A newline before it, so the match is a *top-level* `pub mod` at
        # column 0 and not this block's own indented opening line, which `^`
        # under MULTILINE matches at offset 0 and truncates the block to
        # nothing. That failure is silent: an empty block finds no constants,
        # `hk` comes back empty, and the check goes green having tested
        # nothing — which is the shape of defect this file exists to refuse.
        nxt = re.search(r"\npub mod ", src[start:])
        block = src[start : start + nxt.start()] if nxt else src[start:]
        for name, val in re.findall(r"pub const ([A-Z_0-9]+): i32 = (0x[0-9A-Fa-f]+|\d+);", block):
            if name in names:
                out[name] = int(val, 0)
    return out


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    sites = scan_trace() + scan_compare()
    ledger = load_ledger()
    # The check directory is passed rather than left to `check_files` to
    # derive: under `PROJECTION_DROPS_ROOT` the tree being examined is a copy,
    # and a module that resolved it from its own location would read the real
    # tree's checks while the mutated ones sat unexamined. That is exactly how
    # three falsifier cases went green after this was factored out — the
    # mutation landed and nothing looked at it.
    by_kind = cf.observers(os.path.join(ROOT, "ci/checks/gameplay"))

    if mode == "--list":
        for s in sites:
            print(
                f"{s['stage']:<11} {s['arm']:<28} {s['how']:<24} "
                f"{s['name'] or '<UNMARKED>':<28} {s['file']}:{s['line']}"
            )
        print(f"\n{len(sites)} sites, {len(ledger)} ledger rows")
        return 0

    if mode == "--rank":
        for impact in IMPACTS:
            rows = [r for r in ledger.values() if r.get("impact") == impact]
            print(f"== {impact} ({len(rows)})")
            for row in rows:
                obs = by_kind.get(row["kind"], [])
                # `stage` beside the rank, because the two answer different
                # halves of the same question: `impact` is what the drop costs
                # and `stage` is what it would cost to undo. Reading them apart
                # is how `add_entity.movement` sat filed as an unrecoverable
                # `decode` drop for a day after it stopped being one.
                print(
                    f"   {row['name']:<34} {row['kind']:<24} {len(obs):>3} observers  "
                    f"{row.get('stage', '<none>')}"
                )
                if len(obs) <= 8:
                    for o in obs:
                        print(f"        {o}")
        return 0

    bad = []
    for s in sites:
        if not s["name"]:
            bad.append(
                f"unmarked discard at {s['stage']} {s['arm']} line {s['line']}: "
                f"{s['src']}\n    every discard needs `// DROP: <name> — <why>` above it, "
                f"and a row in {os.path.relpath(LEDGER, ROOT)}"
            )
        elif s["name"] not in ledger:
            bad.append(
                f"undeclared drop `{s['name']}` at {s['stage']} {s['arm']} line {s['line']}"
            )
    # A check whose `observe` list names a kind the recorder cannot emit is a
    # green that no server change can ever move. Nothing in the tree names one
    # today; this is here so that nothing can start to.
    emittable = emittable_kinds()
    hk = housekeeping_ids()
    hk_by_id = {v: k for k, v in hk.items()}
    for kind, checks in sorted(by_kind.items()):
        if kind in emittable:
            continue
        m = re.fullmatch(r"packet:0x([0-9A-F]{2})", kind)
        if m:
            pid = int(m.group(1), 16)
            if pid not in hk_by_id:
                continue
            bad.append(
                f"`{kind}` is `cb::{hk_by_id[pid]}`, which `trace::observe` drops before "
                f"`decode` runs, so it can never appear in any recording under any name — "
                f"observed by {sorted(checks)}"
            )
            continue
        bad.append(
            f"no arm of `trace::decode` emits the kind `{kind}`, observed by "
            f"{sorted(checks)}: that projection is empty in every run"
        )

    # Two classes of row and they are checked differently. A `discard` names a
    # line and must have a marked site; a `normalise` names a *function*,
    # because there is no one line to mark — the loss is the whole of what the
    # function does — and must have its anchor still present in the file it
    # names. Either way, deleting the code goes stale rather than silently
    # un-declaring the loss.
    seen = {s["name"] for s in sites if s["name"]}
    # Where each marker actually sits, as a set: a name with two sites — both
    # `unless_observed` rows have two — must agree with itself as well as with
    # the ledger, because a drop applied at two stages is a different drop
    # from one applied at either.
    site_stages: dict[str, set[str]] = {}
    for s in sites:
        if s["name"]:
            site_stages.setdefault(s["name"], set()).add(s["stage"])
    for name, row in ledger.items():
        if row.get("class") == "normalise":
            anchor, fname = row.get("anchor"), row.get("file")
            if not anchor or not fname:
                bad.append(f"ledger row `{name}` is class normalise with no `anchor`/`file`")
                continue
            path = os.path.join(ROOT, "tools/oracle-runner/src", fname)
            if not os.path.exists(path):
                bad.append(f"ledger row `{name}` anchors in {fname}, which does not exist")
            elif anchor not in open(path, encoding="utf-8").read():
                bad.append(
                    f"stale ledger row `{name}`: {fname} no longer contains its anchor "
                    f"`{anchor}`"
                )
        elif name not in seen:
            bad.append(f"stale ledger row `{name}`: no site in the tree carries that marker")

    for name, row in ledger.items():
        # `stage` against the site that carries the marker. See STAGES above
        # for why this is the recoverability of the field and not a label.
        declared = row.get("stage")
        if declared not in STAGES:
            bad.append(
                f"ledger row `{name}` has stage {declared!r}, not one of {STAGES}"
            )
        elif name in site_stages and site_stages[name] != {declared}:
            found = "/".join(sorted(site_stages[name]))
            bad.append(
                f"ledger row `{name}` files itself as a `{declared}` drop and its site is "
                f"at `{found}`.\n"
                f"    That column is the field's recoverability: a `decode` drop is gone "
                f"from `rest` and needs the recorder changed, a `{found}` drop was "
                f"recorded and needs only the projection changed.\n"
                f"    `ci/projection-drops.py --list` prints the true stage."
            )
        elif declared == "decode" and name not in site_stages:
            # A `normalise` row anchors in `compare.rs`, which runs after the
            # recorder by construction, so it cannot be a decode drop. Nothing
            # in the tree does this today; it is here so that nothing can start
            # to, since an unsited row is exactly the one the check above
            # cannot reach.
            bad.append(
                f"ledger row `{name}` claims stage `decode` but has no site — a row with "
                f"no marked line anchors in the comparator and cannot be a decode drop"
            )
        if row.get("impact") not in IMPACTS:
            bad.append(
                f"ledger row `{name}` has impact {row.get('impact')!r}, "
                f"not one of {IMPACTS}"
            )
        # **A row may name one kind or, for the ambient filter, many.** Until
        # 2026-09-24 `compare::project` carried one `.filter` per ambient kind
        # (`heartbeat_unless_observed`, `entity_position_unless_observed`) and
        # a row per filter. `recorder-housekeeping` replaced them with ONE
        # filter over every `trace::FAMILIES` row of `Class::Ambient`, so its
        # row lists `kinds`, and the list is re-derived from the table here:
        # a family made ambient without the ledger moving is a new drop nobody
        # ranked, which is refusal 2 by another door.
        kinds = row.get("kinds")
        if kinds is not None:
            if row.get("kinds_are") != "ambient":
                bad.append(f"ledger row `{name}` lists `kinds` without `kinds_are = \"ambient\"`")
                continue
            try:
                table = ts.ambient_kinds("\n".join(read(TRACE)))
            except ts.TraceScrapeError as e:
                sys.exit(f"projection-drops: {e}")
            if sorted(kinds) != table:
                bad.append(
                    f"ledger row `{name}` lists the ambient kinds as\n"
                    f"    declared: {sorted(kinds)}\n    trace::FAMILIES: {table}"
                )
            kind = "/".join(sorted(kinds))
            actual = sorted({c for k in kinds for c in by_kind.get(k, [])})
        else:
            kind = row.get("kind")
            if not kind:
                bad.append(f"ledger row `{name}` has no `kind`")
                continue
            actual = sorted(by_kind.get(kind, []))
        count = row.get("observed_by_count")
        if count is None or not count.isdigit():
            bad.append(f"ledger row `{name}` has no numeric `observed_by_count`")
        elif int(count) != len(actual):
            bad.append(
                f"ledger row `{name}` ranks itself against {count} observers of "
                f"`{kind}`, the check files name {len(actual)}: {actual if len(actual) <= 8 else ''}"
            )
        # The names as well as the count, when the list is short enough to be
        # worth reading. A count alone cannot see a rename.
        if "observed_by" in row and sorted(row["observed_by"]) != actual:
            bad.append(
                f"ledger row `{name}` names observers of `{kind}` the check files do not\n"
                f"    declared: {sorted(row['observed_by'])}\n    actual:   {actual}"
            )

    if bad:
        print("projection-drops: REFUSED")
        for b in bad:
            print(f"  {b}")
        return 1
    tally = {k: 0 for k in IMPACTS}
    for r in ledger.values():
        tally[r["impact"]] += 1
    print(
        f"projection-drops: {len(sites)} sites, {len(ledger)} declared drops — "
        + ", ".join(f"{tally[k]} {k}" for k in IMPACTS)
        + f" ({sum(1 for r in ledger.values() if r.get('class') == 'normalise')} of them "
        f"normalisations); {len(emittable)} emittable kinds, {len(hk)} kinds never recorded"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
