#!/usr/bin/env python3
"""corpus-coverage.py — what the gameplay corpus looks at, counted rather than eyeballed.

    ./ci/corpus-coverage.py

Every figure this project has published about the corpus and then had to
withdraw was counted by hand off a printed listing. Two went out on
2026-08-03 — a "43% strict ceiling" that was one comparison at N=2, and
"seventeen unstable" that was really 21 — and the standing rule since is that
a number reaching a tracked doc comes from a tracked script and the
invocation goes in the commit message. This is that script for the three
questions `docs/SERVER.md` answers in prose:

  * which observation kinds does the corpus actually project, and how many
    checks name each;
  * which scenario **actions** does the corpus exercise, and which are
    declared in `Step` but used by nothing — a step nobody sends is a
    mechanism nobody tests, and `finish_dig` sat at zero from the day it was
    written until the day its ordinal was found to be wrong;
  * which `setPlacedBy` classes that write a second block are placement-tested.

**This script cannot tell you what the corpus is missing**, and that is not a
gap in it — every set it counts is read out of our own sources, so a mechanism
this harness has never heard of is invisible to it and 100% of a small set
reads exactly like 100% of a large one. `ci/corpus-surface.py` supplies the
denominator, from the pinned jar's own protocol registration.

# The extraction can fail, and it says so rather than reporting a zero

Both lists below are read out of the Rust sources by regex. A regex that
stops matching returns an empty set, and an empty set makes every count look
like full coverage — the exact "check that could not fail" shape this corpus
keeps finding. So each extraction asserts a non-trivial floor and exits
non-zero if it is not met. A wrong answer here is worse than no answer,
because it would be quoted.
"""

import os
import re
import sys
import tomllib

OPAQUE = re.compile(r"^packet:0x[0-9A-Fa-f]{2}$")

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHECKS = os.path.join(ROOT, "ci", "checks", "gameplay")


def die(msg):
    print(f"corpus-coverage: REFUSED — {msg}", file=sys.stderr)
    sys.exit(2)


def load_checks():
    out = {}
    for fn in sorted(os.listdir(CHECKS)):
        if not fn.endswith(".toml"):
            continue
        with open(os.path.join(CHECKS, fn), "rb") as fh:
            out[fn] = tomllib.load(fh)
    if not out:
        die(f"no checks found under {CHECKS}")
    return out


def declared_steps():
    """The `do = "..."` names `Step` accepts, from the enum's own variants.

    `#[serde(tag = "do", rename_all = "snake_case")]`, so each variant name
    becomes its snake_case form on the wire.
    """
    src = open(os.path.join(ROOT, "tools", "oracle-runner", "src", "scenario.rs")).read()
    body = re.search(r"pub enum Step \{(.*?)\n\}", src, re.S)
    if not body:
        die("could not find `pub enum Step` in scenario.rs — this script has gone stale")
    names = re.findall(r"^\s{4}([A-Z][A-Za-z]*)\s*[\{,\(]", body.group(1), re.M)
    snake = {re.sub(r"(?<!^)(?=[A-Z])", "_", n).lower() for n in names}
    if len(snake) < 8:
        die(f"extracted only {len(snake)} Step variants; the enum has more than that")
    return snake


def recorder_steps():
    """Step variants the **recorder** sends, outside any scenario.

    `record.rs` puts both clients into a known state before every scenario —
    the sneak release, and since 2026-08-05 the container close — because one
    connection serves the whole corpus and a state-shaped verb otherwise leaks
    into the next check. Those steps are sent on every scenario and are not
    written in any `.toml`.

    Without this, such a verb is reported "declared but exercised by nothing",
    which is false and is exactly the kind of stale claim in a tracked report
    that gets acted on: the natural response to that line is to delete the
    variant or to write a scenario that already exists in a better form.
    `close_container` is the first verb whose *only* caller is the reset.
    """
    src = open(os.path.join(ROOT, "tools", "oracle-runner", "src", "record.rs")).read()
    names = re.findall(r"Step::([A-Z][A-Za-z]*)", src)
    return {re.sub(r"(?<!^)(?=[A-Z])", "_", n).lower() for n in names}


def decoder_kinds():
    """Observation kinds `crate::trace` can emit, from its own return strings.

    A kind is the *value* a decode arm yields; `f.insert("window".into(), ..)`
    names a **field** of an observation, not a kind. The first cut of this
    regex matched both and reported 40-odd phantom kinds with zero checks
    each, which reads as a coverage hole that does not exist.

    **The lookbehind that fixed it only fixed the single-line form, and this
    docstring described the trap it was still falling into. Corrected
    2026-08-05.** `(?<!insert\\()` is fixed-width and fires only when
    `insert(` is immediately adjacent to the quote, so

        f.insert(
            "front".into(),

    walked straight past it. Three field names were being reported as decoder
    kinds with zero checks each — `front`, `state` and `rest` — and this
    script exists to feed published figures, so that was three phantom
    coverage holes in a tracked document.

    Python's `re` has no variable-length lookbehind, so the `insert(...)`
    arguments are neutralised first and the match is then unconditional.
    """
    src = open(os.path.join(ROOT, "tools", "oracle-runner", "src", "trace.rs")).read()
    # Strip the field-name argument out of every `insert(` call, single-line
    # or not, then match what is left.
    without_fields = re.sub(r'insert\(\s*"[a-z_]+"\.into\(\)', "insert(", src)
    kinds = set(re.findall(r'"([a-z_]+)"\.into\(\)', without_fields))
    # **A kind yielded through a named constant is still a kind**, and missing
    # one is the dangerous direction: the caller prints "NOT DECODED —
    # projected but the decoder never emits it", which says a live projection
    # is dead. Found 2026-08-04, on `move_entity_heartbeat`: `trace.rs` yields
    # it as `HEARTBEAT.into()`, so the literal regex above never saw it and
    # this script had been telling anyone who ran it that
    # `520-a-standing-player-is-rebroadcast-on-a-timer` watches a kind nothing
    # produces. It does not — that check is `match`.
    #
    # This script exists so that no corpus figure is hand-counted. A wrong
    # automated figure is worse than a hand-counted one, because nobody
    # re-checks it.
    consts = dict(re.findall(r'const\s+([A-Z_][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([a-z_]+)"', src))
    for name, value in consts.items():
        if re.search(rf'\b{name}\.into\(\)', src):
            kinds.add(value)
    kinds -= {"rest", "inline"}
    if len(kinds) < 15:
        die(f"extracted only {len(kinds)} kinds from trace.rs; it decodes more than that")
    return kinds


def placed_by_second_block():
    """setPlacedBy classes writing a block at an offset other than their own."""
    import json

    path = os.path.join(ROOT, "data", "vanilla", "set_placed_by.json")
    if not os.path.exists(path):
        return None  # gitignored and generated; absent is not an error here
    tbl = json.load(open(path))["table"]
    out = {}
    for e in tbl:
        offs = {tuple(w["at"]) for s in e["states"] for w in s.get("writes", [])}
        offs.discard((0, 0, 0))
        if offs:
            out.setdefault(e["class"], []).append(e["block"])
    return out


def main():
    checks = load_checks()
    steps = declared_steps()
    kinds = decoder_kinds()

    used_steps, used_kinds = {}, {}
    for fn, c in checks.items():
        for s in c.get("setup", []) + c.get("step", []):
            used_steps.setdefault(s["do"], []).append(fn)
        for k in c.get("observe", []):
            used_kinds.setdefault(k, []).append(fn)

    print(f"corpus: {len(checks)} checks under ci/checks/gameplay")
    print()

    print("== observation kinds, by number of checks projecting them ==")
    for k in sorted(kinds | set(used_kinds)):
        n = len(used_kinds.get(k, []))
        note = ""
        if OPAQUE.match(k):
            # **NOT a decoder hole, and this line used to say it was.**
            # `trace::observe` records a packet with no decoder arm as
            # `packet:0xNN` with its whole body digested into `rest` — see the
            # comment there about `add_entity` going unrecorded on 2026-08-02.
            # So a check projecting one is comparing every byte of it, more
            # strictly than a decoded packet, and the old label
            # ("the decoder never emits it") was false in the dangerous
            # direction: it read as a dead projection somebody should delete.
            # `040` has projected `packet:0x4A` deliberately since it was
            # written. Which packet the id is, and whether vanilla actually
            # sends it here, is `ci/corpus-surface.py`'s question.
            note = "  (opaque — no decoder arm, so the whole body is compared as a digest)"
        elif k not in kinds:
            note = "  (NOT DECODED — projected but the decoder never emits it)"
        elif n == 0:
            note = "  (decoded but no check looks at it)"
        print(f"  {k:26s} {n:3d}{note}")
    print()

    print("== scenario actions, by number of checks using them ==")
    by_recorder = recorder_steps()
    for s in sorted(steps | set(used_steps)):
        n = len(used_steps.get(s, []))
        note = ""
        if s not in steps:
            note = "  (NOT A DECLARED STEP — would fail to parse)"
        elif n == 0 and s in by_recorder:
            note = "  (no check names it; sent by record.rs before every scenario)"
        elif n == 0:
            note = "  (declared but exercised by nothing)"
        print(f"  {s:26s} {n:3d}{note}")
    print()

    # The three families `docs/SERVER.md` makes a claim about in prose. Counted
    # as **distinct checks projecting at least one kind in the family**, which
    # is the thing the prose asserts; summing the per-kind column would double
    # count a check that names two of them.
    families = {
        "entities": [
            "add_entity",
            "remove_entities",
            "set_entity_data",
            "set_entity_motion",
            "entity_position",
            "move_entity",
            "take_item_entity",
            "entity_event",
        ],
        "inventory": [
            "container_set_slot",
            "container_set_content",
            "set_equipment",
            "open_screen",
            # Added 2026-08-12 with their decoder arms (branch
            # `trace-itemstack-legible`). Both were `packet:0x60`/`packet:0x6C`
            # before, and a literal `packet:` kind cannot join a family here —
            # so checks 607 and 720 were observing the cursor and scoring
            # nothing for it.
            "set_cursor_item",
            "set_player_inventory",
        ],
        "block entities": ["block_entity_data", "block_event", "open_sign_editor"],
    }
    print("== families docs/SERVER.md makes a claim about ==")
    for fam, ks in families.items():
        hit = set()
        for k in ks:
            hit.update(used_kinds.get(k, []))
        named = sorted(k for k in ks if used_kinds.get(k))
        print(f"  {fam:16s} {len(hit):3d} of {len(checks)} checks   via {', '.join(named) or '(nothing)'}")
    print()

    fams = placed_by_second_block()
    if fams is None:
        print("== setPlacedBy second-block families: data/vanilla/set_placed_by.json absent, skipped ==")
    else:
        print("== setPlacedBy classes that write a second block ==")
        placed = {}
        for fn, c in checks.items():
            for s in c.get("setup", []) + c.get("step", []):
                if s["do"] == "hold":
                    placed.setdefault(s["item"], []).append(fn)
        for cls, blocks in sorted(fams.items()):
            hit = [b for b in blocks if b in placed]
            mark = "tested" if hit else "NO PLACEMENT CHECK"
            shown = ", ".join(b.replace("minecraft:", "") for b in hit[:3])
            print(f"  {cls:26s} {len(blocks):3d} blocks  {mark}{'  via ' + shown if hit else ''}")
        tested = sum(1 for c, b in fams.items() if any(x in placed for x in b))
        print(f"  -> {tested} of {len(fams)} such classes are placement-tested")


if __name__ == "__main__":
    main()
