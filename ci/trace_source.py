#!/usr/bin/env python3
"""Facts scraped out of `tools/oracle-runner/src/trace.rs` — one implementation.

    import trace_source as ts
    ts.housekeeping_names(src)   # the `cb::` names `observe` drops at RECORD time

Underscore in the filename because it is imported as a module, the same reason
`ci/check_files.py` and `ci/trace_vocabulary.py` have one.

# WHY THIS EXISTS

`ci/check_files.py`'s header makes the argument and it applies again here: *two
implementations of one scrape are two answers waiting for an occasion.* Three
scripts were matching `fn is_housekeeping` with their own regex —
`ci/corpus-surface.py`, `ci/projection-drops.py`, and `ci/never-recorded.py`,
which made it three on 2026-08-19 — and they did not agree about what to do when
the scrape stopped matching. That disagreement is the whole risk: a scrape that
silently returns the empty set makes every claim resting on it trivially true,
and this particular set is the list of packets no recording contains, so an
empty answer reads as "nothing is hidden".

`ci/projection-drops.py` has already paid for that once. Its `housekeeping-kind`
falsifier case passed vacuously for a while because a missing `ids.rs` made the
id table come back empty, every `packet:0xNN` looked unresolvable, and the check
printed `0 kinds never recorded` and exited 0 having tested nothing.

So the refusals live here rather than in each caller. Until 2026-09-24 they
were: a missing function is fatal, an empty match is fatal, and fewer than
eight names is fatal. The function they read is gone and the true answer is
now empty; the replacement refusals are described above `housekeeping_names`.
`ci/corpus-surface.py` no longer calls this — it reads `trace::FAMILIES`'
ambient rows, which is the question it was actually asking.
"""

from __future__ import annotations

import re

# **The record-time drop set, read from `trace::observe` since 2026-09-24.**
#
# Until then this read `fn is_housekeeping`, which dropped twelve clientbound
# kinds before `decode` ran, and refused a match of fewer than eight names on
# the ground that twelve was the true count. `recorder-housekeeping` deleted
# that function: every packet is recorded, and the classification that used to
# drop them is `trace::FAMILIES`, applied at PROJECTION time, where a scenario
# naming a kind gets it back. So the true count is now ZERO and a floor of
# eight would refuse the correct answer.
#
# What protects the empty set from being a scrape that stopped matching is two
# refusals rather than a floor: `observe` must be found (a renamed function is
# fatal, not empty), and `pub const FAMILIES` must be found (the table that
# replaced the drop list; if it is gone, "nothing is dropped" is no longer a
# claim anything else in the tree backs). `ci/ambient-falsify.py` R1 refuses any
# `return None` in `observe` at all, so a non-empty answer here can only come
# from a mutated tree — which is exactly what the falsifiers use it for.
#
# The shape read is the only one a record-time drop can take now: every `cb::`
# name in the condition of an `if` whose body is `return None;`.
_OBSERVE = re.compile(r"pub fn observe\(.*?\n\}\n", re.S)
_IF_NONE = re.compile(r"\bif\s+([^{]*?)\{\s*return None;", re.S)
_CB = re.compile(r"cb::([A-Z_0-9]+)")


class TraceScrapeError(RuntimeError):
    """The scrape stopped matching. Never a reason to return an empty set."""


def housekeeping_names(src: str) -> list[str]:
    """The `cb::` constant names `trace::observe` drops at record time, sorted.

    The name is kept from when this read `is_housekeeping`, because it is what
    the callers ask for: the kinds no recording can contain under any name.

    Takes the source text rather than a path on purpose: two of the callers
    run under a `*_ROOT` environment override pointing at a mutated copy of
    the tree, and a helper that resolved the path itself would read the real
    tree while the mutation sat unexamined. That is exactly how three
    falsifier cases in `ci/projection-drops.py` went green after a refactor.
    """
    m = _OBSERVE.search(src)
    if not m:
        raise TraceScrapeError(
            "could not find `pub fn observe` in trace.rs — it moved or was renamed"
        )
    if "pub const FAMILIES" not in src:
        raise TraceScrapeError(
            "`pub const FAMILIES` is gone from trace.rs, so an empty record-time "
            "drop set is no longer backed by the table that replaced the drop list"
        )
    names = set()
    for cond in _IF_NONE.findall(m.group(0)):
        names |= set(_CB.findall(cond))
    return sorted(names)


# `trace::FAMILIES`' `Class::Ambient` rows, by recorded kind. Same refusals as
# above: a table that cannot be found is fatal, and so is a table that parses
# to fewer rows than any real version has had (fourteen as of 2026-09-24; the
# floor catches a regex that stopped matching, not a row removed on purpose).
MIN_FAMILIES = 8
_FAMILIES = re.compile(r"pub const FAMILIES: &\[Family\] = &\[.*?\n\];", re.S)
_ROW = re.compile(r"Family \{(.*?)\n    \}", re.S)


def ambient_kinds(src: str) -> list[str]:
    """The `Obs::kind` of every `trace::FAMILIES` row whose class is Ambient."""
    tbl = _FAMILIES.search(src)
    if not tbl:
        raise TraceScrapeError("could not find `pub const FAMILIES` in trace.rs")
    rows = _ROW.findall(tbl.group(0))
    if len(rows) < MIN_FAMILIES:
        raise TraceScrapeError(
            f"FAMILIES parsed to {len(rows)} rows, floor is {MIN_FAMILIES} — the scrape has stopped matching"
        )
    out = []
    for body in rows:
        if "Class::Ambient" not in body:
            continue
        k = re.search(r'kind:\s*(?:"([a-z_0-9]+)"|([A-Z_]+))', body)
        if not k:
            raise TraceScrapeError("a FAMILIES row names no kind")
        name = k.group(1)
        if name is None:
            c = re.search(r'pub const %s: &str = "([a-z_0-9]+)"' % k.group(2), src)
            if not c:
                raise TraceScrapeError(f"FAMILIES names {k.group(2)}, which is not a &str const")
            name = c.group(1)
        out.append(name)
    return sorted(out)
