#!/usr/bin/env python3
"""What the chunk-send path can and cannot put into a recording."""

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/chunk-send-unrecorded.py            # check; non-zero on drift
#     ci/chunk-send-unrecorded.py --show     # the table, no verdict
#
# Refuses when a chunk-send packet kind is dropped at RECORD time by
# `trace::observe` and has no row in `ci/checks/chunk-send-unrecorded.toml`,
# when a row names a kind that is no longer dropped, or when a row's id
# disagrees with `crates/rustmc-proto/src/ids.rs`.
#
# WHAT BREAKS IF YOU IGNORE THIS: a kind silently returns to the projection's
# drop list and every check that reads it goes quietly vacuous. That already
# happened to LEVEL_CHUNK_WITH_LIGHT — see below.
# ===== end interface; everything below is why =====

# # THE DEFECT THIS EXISTS TO CATCH, AND IT IS NOT HYPOTHETICAL
#
# `docs/PARITY-ROADMAP.md` §31 established that the chunk payload is the only
# channel 29 block entity types are on, and §32 fixed `World::chunk` to build
# that payload's block-entity list. Both were written against a projection in
# which `LEVEL_CHUNK_WITH_LIGHT` was a member of `trace::is_housekeeping` —
# that is, against a packet **no recording contained**. The work was correct
# and the channel it opened was, from the corpus's side, indistinguishable
# from not existing.
#
# That is this project's central defect class wearing a new hat: not a check
# whose green cannot be turned red, but a check whose *subject* never arrives.
# `ci/corpus-falsifiable.py` cannot catch it, because mutating the subject side
# of a trace pair moves nothing when the packet was filtered before the trace
# was written — which reads as tier 1, "only deleting the whole scenario moves
# it", the same signature as a scenario that measures nothing.
#
# So the ledger records the kinds still in that state and this script refuses
# their number changing in either direction without the ledger moving with it.
# Since 2026-09-24 (`recorder-housekeeping`) that number is ZERO: the five rows
# it held were retired by this script's own STALE-ROW rule when the recorder
# stopped dropping anything at record time.
#
# # WHY THE CHUNK-SEND SET IS PINNED IN THE LEDGER AND NOT DERIVED
#
# "Is this kind part of the chunk-send path" is a judgement about the server,
# not a fact in a table, so deriving it would mean inventing a heuristic and
# then trusting it. It is pinned in `chunk_send_kinds` below with the vanilla
# method that sends each one, and the pin is itself checked: every name must
# resolve in `ids.rs`, so a rename cannot leave a stale entry behind.

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TRACE = ROOT / "tools" / "oracle-runner" / "src" / "trace.rs"
IDS = ROOT / "crates" / "rustmc-proto" / "src" / "ids.rs"
LEDGER = ROOT / "ci" / "checks" / "chunk-send-unrecorded.toml"

IMPACTS = {"decides", "armed", "note"}

# The chunk-send path, and the vanilla method that emits each kind. Read off
# the pinned server-26.2.jar with `javap -p -c -constants`.
CHUNK_SEND_KINDS = {
    "LEVEL_CHUNK_WITH_LIGHT": "PlayerChunkSender.sendChunk",
    "CHUNK_BATCH_START": "PlayerChunkSender.sendNextChunks",
    "CHUNK_BATCH_FINISHED": "PlayerChunkSender.sendNextChunks",
    "FORGET_LEVEL_CHUNK": "PlayerChunkSender.dropChunk",
    "LIGHT_UPDATE": "the light engine, outside sendChunk",
    "SET_CHUNK_CACHE_CENTER": "the chunk source, on a centre change",
}


def housekeeping_kinds(text: str) -> set[str]:
    """The `cb::NAME`s `trace::observe` drops at record time.

    # THIS READ `is_housekeeping` UNTIL 2026-09-24, AND THAT FUNCTION IS GONE

    `recorder-housekeeping` deleted `is_housekeeping`: every packet is now
    recorded, and the classification that used to drop five of this path's
    kinds before the trace was written is `trace::FAMILIES`, applied at
    PROJECTION time, where a scenario naming a kind gets it back
    (`546`/`547`/`548` do). `ci/ambient-falsify.py` R1 refuses any
    `return None` in `observe`. So the only way a chunk-send kind can reach no
    recording now is an early `return None` in `observe` under a test naming
    it, and that is the shape read here: every `cb::NAME` in the condition of
    an `if` whose body is `return None;`. The kind-level question this guard
    asks is unchanged; only where the answer lives moved.
    """
    m = re.search(r"pub fn observe\(.*?\n}\n", text, re.S)
    if not m:
        raise SystemExit(
            "chunk-send-unrecorded: could not find `observe` in "
            f"{TRACE.relative_to(ROOT)} — this guard reads it by shape, so a "
            "rewrite of that function must be reflected here rather than "
            "silently passing"
        )
    out = set()
    for cond in re.findall(r"\bif\s+([^{]*?)\{\s*return None;", m.group(0), re.S):
        out |= set(re.findall(r"cb::([A-Z0-9_]+)", cond))
    return out


def cb_ids(text: str) -> dict[str, int]:
    """`ids::play::cb`'s constants. The `cb` module is the second `mod cb`."""
    out = {}
    for mod in re.finditer(r"pub mod cb \{(.*?)\n    \}", text, re.S):
        for name, val in re.findall(
            r"pub const ([A-Z0-9_]+): i32 = (0x[0-9a-fA-F]+|\d+);", mod.group(1)
        ):
            out.setdefault(name, int(val, 0))
    return out


def main() -> int:
    trace = TRACE.read_text()
    dropped = housekeeping_kinds(trace)
    ids = cb_ids(IDS.read_text())
    ledger = tomllib.loads(LEDGER.read_text())
    rows = {r["name"]: r for r in ledger.get("kind", [])}

    show = "--show" in sys.argv
    errs = []

    # The pin must resolve, so a rename cannot leave a stale entry.
    for name in CHUNK_SEND_KINDS:
        if name not in ids:
            errs.append(
                f"PIN-UNRESOLVED {name} is pinned as a chunk-send kind and is "
                f"not a `cb::` constant in {IDS.relative_to(ROOT)}"
            )

    unrecorded = {k for k in CHUNK_SEND_KINDS if k in dropped}

    for name in sorted(unrecorded):
        if name not in rows:
            errs.append(
                f"UNLEDGERED {name} is dropped by trace::observe and has no "
                f"row in {LEDGER.relative_to(ROOT)}. It reaches no recording, "
                "so no check can observe it and none can be written to."
            )
    for name, row in sorted(rows.items()):
        if name not in CHUNK_SEND_KINDS:
            errs.append(f"NOT-CHUNK-SEND {name} has a row and is not a pinned chunk-send kind")
            continue
        if name not in dropped:
            errs.append(
                f"STALE-ROW {name} has a row saying it is unrecorded, and "
                "trace::observe no longer drops it — the row is now false. "
                "Delete it; the kind is observable."
            )
        if row.get("recorded") is not False:
            errs.append(f"BAD-ROW {name} must carry `recorded = false`")
        if row.get("impact") not in IMPACTS:
            errs.append(f"BAD-IMPACT {name} has impact={row.get('impact')!r}, not one of {sorted(IMPACTS)}")
        if name in ids and row.get("id") != ids[name]:
            errs.append(
                f"ID-DRIFT {name} row says id={row.get('id')}, "
                f"{IDS.relative_to(ROOT)} says {ids[name]}"
            )

    if show or errs:
        print(f"chunk-send path: {len(CHUNK_SEND_KINDS)} kinds, "
              f"{len(unrecorded)} unrecorded, {len(rows)} ledgered")
        for name in sorted(CHUNK_SEND_KINDS):
            state = "UNRECORDED" if name in unrecorded else "recorded"
            imp = rows.get(name, {}).get("impact", "-")
            print(f"  {name:<24} {state:<11} impact={imp:<8} {CHUNK_SEND_KINDS[name]}")
    if errs:
        print()
        for e in errs:
            print(f"chunk-send-unrecorded: {e}")
        return 1
    if not show:
        print(
            f"chunk-send-unrecorded: OK — {len(unrecorded)} of "
            f"{len(CHUNK_SEND_KINDS)} chunk-send kinds unrecorded, all ledgered"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
