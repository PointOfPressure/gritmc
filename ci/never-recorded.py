#!/usr/bin/env python3
"""Rank the twelve packet kinds that never enter a recording, and refuse a new one.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/never-recorded.py            check the ledger against trace.rs and the jar
#     ci/never-recorded.py --list     the derived table, no checking
#
# WHAT BREAKS IF YOU IGNORE THIS: `trace::observe` returning `None`
# before `decode` runs, so a kind named there is invisible to every check, every
# floor and every instrument in this repository — and unlike a dropped *field*,
# nothing records that it was ever there. `ci/projection-drops.py` refuses a
# check that names one; this file is what says whether that refusal is costing
# anything.
# ===== end interface; everything below is why =====

# WHY A SECOND LEDGER RATHER THAN A ROW IN THE FIRST

`ci/checks/projection-drops.toml` enumerates dropped **fields**: something the
decoder saw and did not emit. These twelve are a different loss and a worse one.
`observe` returns `None` before `decode` is reached, so there is no observation,
no `rest` digest, and no `packet:0xNN` fallback — the packet does not appear in
the recording under any name at all. A field drop leaves a hole you can point
at. This leaves nothing.

The ledger's own header makes the ranking axis for fields "what could this
decide". The axis here is different and simpler, because the question people
actually ask about these is *"is anything being hidden?"*:

  * `vacant`      — **no class in the pinned jar constructs the packet.** A
    vanilla server cannot send it, so refusing it costs nothing and can never
    cost anything until the jar changes. This is the only verdict that is a
    statement about the reference implementation rather than about us.
  * `clock`       — sent, and its payload is a clock. Two runs of one scenario
    against one jar disagree, so no value comparison is possible for the same
    reason `player_chat.timestamp` is a `note`.
  * `recoverable` — sent, and its payload is decided by what the server did.
    A check could compare it if the recorder stopped refusing the id. **These
    are the ones that cost something today.**
  * `elsewhere`   — the chunk-send path. Six of the twelve, owned by
    `origin/chunk-send-path`, listed here so that the count is complete and
    ranked there rather than twice.

# THE DERIVATION, AND WHY IT IS A CONSTANT-POOL SCAN

For each kind the ledger names a vanilla class, and this script counts the
classes **outside `net/minecraft/network/protocol/`** whose constant pool
mentions it. That package is excluded because every packet is referenced three
times by plumbing that sends nothing: the listener interface that declares its
`handle`, the `*PacketTypes` registry, and `GameProtocols` wiring the codec. A
scan that kept those would report four senders for a packet with none, which is
the answer that makes the whole verdict useless.

Constant-pool containment **over-approximates**: a class that only mentions the
type still counts. That is the safe direction and it is chosen deliberately.
The verdict this file exists to protect is `vacant`, and an over-approximation
that returns zero is a strong claim — nothing outside the protocol plumbing so
much as names the class. An under-approximation could report `vacant` for a
packet something quietly sends, which is exactly the defect.

`PING` and `TAB_LIST` are the two, and neither is a technicality: `ClientGame
PacketListener.handleTabListCustomisation` exists, `GamePacketTypes` holds the
id, `GameProtocols` registers the codec, and no server class ever builds one.
A tab-list header is set by plugins; vanilla has no code path that sends it.

# ABSENT JAR IS A REFUSAL, NOT AN EMPTY ANSWER

Same discipline as `ci/corpus-surface.py` and for the same reason
`ci/projection-drops.py::housekeeping_ids` learnt it: with no jar every scan
returns the empty set, every `vacant` claim is trivially satisfied, and the
check prints a green having tested nothing. That is the shape of defect this
directory exists to refuse, so it would not do to have one here.

Its own red-when-broken evidence is `ci/never-recorded-falsify.sh`.
"""

import os
import re
import sys
import zipfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import trace_source as ts  # noqa: E402 — must follow the sys.path line

ROOT = os.environ.get("NEVER_RECORDED_ROOT") or os.path.dirname(
    os.path.dirname(os.path.abspath(__file__))
)
TRACE = os.path.join(ROOT, "tools/oracle-runner/src/trace.rs")
LEDGER = os.path.join(ROOT, "ci/checks/never-recorded.toml")
JAR = os.environ.get("NEVER_RECORDED_JAR") or os.path.join(
    ROOT, "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
)

VERDICTS = ("vacant", "clock", "recoverable", "elsewhere")

# Everything under this package references every packet it plumbs. See the
# module docs: keeping it would report senders for packets with none.
PLUMBING = "net/minecraft/network/protocol/"


def die(msg, code=2):
    print(f"never-recorded: REFUSED — {msg}", file=sys.stderr)
    sys.exit(code)


def housekeeping_names():
    """The `cb::` constant names `trace::observe` drops at record time (it was `is_housekeeping` until 2026-09-24).

    Scraped by `ci/trace_source.py`, which is where the refusals live: this was
    the third copy of that regex in `ci/` and the three did not agree about what
    an empty match meant.
    """
    if not os.path.exists(TRACE):
        die(f"cannot read {TRACE}")
    try:
        return ts.housekeeping_names(open(TRACE, encoding="utf-8").read())
    except ts.TraceScrapeError as e:
        die(str(e))


def load_ledger():
    if not os.path.exists(LEDGER):
        die(f"no ledger at {LEDGER}")
    import tomllib

    with open(LEDGER, "rb") as fh:
        doc = tomllib.load(fh)
    rows = {}
    for row in doc.get("kind", []):
        if "name" not in row:
            die("a ledger row has no `name`")
        rows[row["name"]] = row
    return rows


def senders(rows):
    """{kind: sorted class names outside the protocol package that mention its class}.

    One pass over the jar for all twelve, because opening it twelve times to
    answer twelve questions about the same 7,452 entries is the kind of thing
    that makes a gate too slow to keep.
    """
    if not os.path.exists(JAR):
        die(
            f"pinned jar not found at {JAR} — every `vacant` verdict below would be "
            f"satisfied by an empty scan, so this is a refusal and not a zero"
        )
    want = {}
    for name, row in rows.items():
        cls = row.get("class")
        if not cls:
            die(f"ledger row `{name}` names no `class`")
        want[name] = cls.replace(".", "/")

    out = {k: [] for k in want}
    # Encoded once. The inner test runs once per (class, target) pair — 89,424
    # of them for this jar — and calling `str.encode` inside that loop was
    # costing more than decompressing the 42 MB it searches.
    want_b = {name: cls.encode() for name, cls in want.items()}
    # One compiled alternation as a prefilter. All but a handful of the 7,452
    # classes mention none of the twelve, and for those the twelve separate
    # substring searches are pure waste; a single scan rejects them at C speed
    # and the per-target loop then runs only on the classes that hit.
    any_target = re.compile(b"|".join(re.escape(v) for v in set(want_b.values())))
    present = set()
    with zipfile.ZipFile(JAR) as z:
        entries = [n for n in z.namelist() if n.endswith(".class")]
        if len(entries) < 1000:
            die(f"{JAR} holds {len(entries)} classes — that is not the server jar")
        for entry in entries:
            stem = entry[:-6]
            present.add(stem)
            if entry.startswith(PLUMBING):
                continue
            data = z.read(entry)
            if not any_target.search(data):
                continue
            for name, cls in want_b.items():
                if stem != want[name] and cls in data:
                    out[name].append(stem.replace("/", "."))
    for name, cls in want.items():
        if cls not in present:
            die(
                f"ledger row `{name}` names `{cls.replace('/', '.')}`, which is not in the "
                f"pinned jar — the class moved or was renamed and every verdict resting "
                f"on it is stale"
            )
    return {k: sorted(v) for k, v in out.items()}


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    hk = housekeeping_names()
    rows = load_ledger()
    found = senders(rows)

    if mode == "--list":
        for name in hk:
            row = rows.get(name, {})
            print(
                f"{name:<24} {row.get('verdict', '<UNRANKED>'):<12} "
                f"{len(found.get(name, [])):>2} senders  {row.get('class', '')}"
            )
        return 0

    bad = []
    # The set itself. A kind added to `is_housekeeping` without a row is a
    # packet removed from every recording that nobody ranked, which is the
    # defect this file exists for.
    for name in hk:
        if name not in rows:
            bad.append(
                f"`cb::{name}` is dropped by `trace::observe` at record time and has no ledger row — "
                f"it is absent from every recording and nobody has said what that costs"
            )
    for name in rows:
        if name not in hk:
            bad.append(
                f"stale ledger row `{name}`: `trace::observe` no longer drops it, so "
                f"it reaches `decode` and this row describes a loss that stopped happening"
            )

    for name, row in rows.items():
        verdict = row.get("verdict")
        if verdict not in VERDICTS:
            bad.append(f"ledger row `{name}` has verdict {verdict!r}, not one of {VERDICTS}")
            continue
        if name not in found:
            continue
        actual = found[name]
        declared = sorted(row.get("sent_by", []))
        if declared != actual:
            bad.append(
                f"ledger row `{name}` names the classes that build it as\n"
                f"    declared: {declared}\n    actual:   {actual}"
            )
        # The verdict that is a claim about the jar, checked against the jar.
        if verdict == "vacant" and actual:
            bad.append(
                f"ledger row `{name}` is ranked `vacant` — nothing in the jar builds it — "
                f"and {len(actual)} class(es) now do: {actual}.\n"
                f"    A vanilla server can send it, so refusing the id in "
                f"`trace::observe` now hides something."
            )
        if verdict != "vacant" and not actual:
            bad.append(
                f"ledger row `{name}` is ranked `{verdict}`, which asserts the packet is "
                f"sent, and no class outside the protocol plumbing builds it"
            )

    if bad:
        print("never-recorded: REFUSED")
        for b in bad:
            print(f"  {b}")
        return 1

    tally = {v: 0 for v in VERDICTS}
    for row in rows.values():
        tally[row["verdict"]] += 1
    print(
        f"never-recorded: {len(rows)} kinds never recorded — "
        + ", ".join(f"{tally[v]} {v}" for v in VERDICTS)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
