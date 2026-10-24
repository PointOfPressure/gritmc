#!/usr/bin/env python3
"""serverbound-surface.py — the half of the play protocol the corpus cannot see.

    ./ci/serverbound-surface.py
    ./ci/serverbound-surface.py --selftest

`ci/corpus-surface.py` reads **both** halves of the protocol out of
`GameProtocols` — the 141 clientbound and 69 serverbound packet types, in id
order — and then censuses only the clientbound one. `docs/PARITY-ROADMAP.md`
§21's UNIT 1 does the same. This script is the missing half.

# Why the asymmetry is not cosmetic

Clientbound is what vanilla *says*, and the corpus compares it byte for byte.
Serverbound is what a client can *do to us*, and it fails differently: a
serverbound packet this server ignores produces no wrong byte, because it
produces no byte at all. Nothing we emit changes, so the recorder sees the
same silence from both servers and the comparison reports `match`.

**That is a check which cannot fail, in the one direction the corpus is
structurally unable to see.** The recorder only ever calls `trace::observe`
on the read path (`client.rs`, inbound frames); no serverbound packet is
recorded by anything, ever. So unlike the clientbound `emitted` column there
is no measurement available here at all — every set below is read out of the
jar or out of our own sources, and the script says so rather than implying a
sampled number.

# The five sets, narrowing

    jar         every serverbound play packet type          (GameProtocols)
    named       we have an id constant for it               (rustmc-proto)
    sendable    the harness can put it on the wire          (client.rs)
    sent        at least one scenario's steps send it       (ci/checks + verbs)
    decoded     the server has a dispatch arm for it        (rustmc-server)

and then `decoded` splits three ways, which is the split that matters:

    handled     an arm, and the result reaches an observable sink
    accepted    an arm, and nothing downstream reads what it wrote
    discarded   an arm that drops the body on purpose

`accepted` is the dangerous bucket. A packet there decodes cleanly, writes a
field, and no line of this server ever reads that field — so the arm could be
deleted and every check in the repository would stay green.

# What is derived and what is declared

Everything is derived. The one place a human judgement enters is
`ACCEPTED_REASON`: when the call-graph analysis finds no observable sink
behind an arm, the packet must appear there with a reason, or this script
refuses. That is the opposite of a caveat list — the table cannot go stale
in the direction that flatters us, because a packet that *gains* a sink drops
out of the analysis's no-sink set and is then reported as an unnecessary
entry, and a packet that *loses* one refuses until somebody writes down why.

# The extraction can fail, and it says so rather than reporting a zero

Same discipline as `ci/corpus-surface.py`, whose jar and `ids.rs` extractions
this script imports rather than reimplements — so the fatal id cross-check
against the pinned jar covers this census too. Every scrape added here
asserts a floor and exits 2 rather than print a number it cannot stand
behind. `--selftest` exercises each of those refusals.
"""

import argparse
import importlib.util
import os
import re
import sys
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHECKS = os.path.join(ROOT, "ci", "checks", "gameplay")
CLIENT_RS = os.path.join(ROOT, "tools", "oracle-runner", "src", "client.rs")
SCENARIO_RS = os.path.join(ROOT, "tools", "oracle-runner", "src", "scenario.rs")
SERVER_SRC = os.path.join(ROOT, "crates", "rustmc-server", "src")

# `crate::player::ids` is a second, hand-written id table, because the movement
# decoder is a pure function in `player.rs` that does not depend on
# `rustmc-proto`. It is the same protocol, so both tables are read and unioned;
# `ci/corpus-surface.py`'s `proto_drift` already refuses if either disagrees
# with the jar.
PLAYER_IDS_RS = os.path.join(SERVER_SRC, "player.rs")

# A verb that legitimately sends nothing. Declared, so that a verb which stops
# sending is a refusal rather than a silently smaller reach.
SILENT_VERBS = {"wait"}

# Packets whose arm the sink analysis finds nothing behind. A packet here is
# reported as `accepted`; a packet the analysis clears is reported as
# `handled` and must NOT be here.
ACCEPTED_REASON = {
    "ACCEPT_TELEPORTATION": (
        "writes PlayerState::acknowledged_teleport, which no line outside "
        "player.rs's own unit tests reads"
    ),
}


def die(msg):
    print(f"serverbound-surface: REFUSED — {msg}", file=sys.stderr)
    sys.exit(2)


def _corpus_surface():
    """Import `ci/corpus-surface.py` — the jar and ids.rs extractions live there.

    Reused rather than copied for the reason `data/vanilla` is copied rather
    than shared: two scrapes of one file drift, and the one that drifts is
    whichever nobody re-checks. The dash in the filename is why this is
    importlib and not an import statement.
    """
    p = os.path.join(ROOT, "ci", "corpus-surface.py")
    spec = importlib.util.spec_from_file_location("corpus_surface", p)
    if spec is None or spec.loader is None:
        die(f"could not import {p}")
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


# --------------------------------------------------------------------------
# our own sets — each scoped to the PLAY protocol, which is the fix
# --------------------------------------------------------------------------

PLAY_SB = re.compile(r"ids::play::sb::([A-Z_0-9]+)")


def harness_sends(src=None):
    """Serverbound **play** packets `client.rs` can put on the wire.

    # Why this is not `corpus-surface.py`'s `client_sends`

    That one matches `sb::([A-Z_0-9]+)` anywhere in `client.rs`, and
    `client.rs` drives all five protocols. `ids::config::sb::CLIENT_INFORMATION`
    therefore reported `CLIENT_INFORMATION` as a serverbound *play* packet the
    harness can send.

    It cannot. `client_information` is registered in **both** protocols and
    both registrations name the same class — the jar pairs the play type with
    `common/ServerboundClientInformationPacket`, which is the class the
    configuration protocol uses too — so the collision is real rather than a
    coincidence of names. What differs is the id: play 0x0E against
    configuration's own, and `ids::play::sb` carries no constant for it at all.
    The harness sends the configuration one, during configuration, and has no
    way to send the play one. `KEEP_ALIVE` is the same collision with the
    opposite answer, which is why the fix is protocol scoping and not an
    exclusion list: it is registered in both, and the harness really does send
    the play one.

    One packet of over-count out of 69 is small; the direction is not. It is
    the direction that reports reach this harness does not have.
    """
    if src is None:
        src = open(CLIENT_RS).read()
    names = set(PLAY_SB.findall(src))
    if len(names) < 15:
        die(f"client.rs mentions only {len(names)} ids::play::sb ids")
    return names


def step_variants(src=None):
    """The `Step` enum's variants, as the snake_case `do =` names."""
    if src is None:
        src = open(SCENARIO_RS).read()
    m = re.search(r"pub enum Step \{(.*?)\n\}\n", src, re.S)
    if not m:
        die("could not find `pub enum Step` in scenario.rs")
    body = re.sub(r"^\s*//.*$", "", m.group(1), flags=re.M)
    names = re.findall(r"^    ([A-Z][A-Za-z0-9]*)\s*[\{,]", body, re.M)
    if len(names) < 10:
        die(f"Step has only {len(names)} variants — the enum scrape has gone stale")
    return {n: re.sub(r"(?<!^)(?=[A-Z])", "_", n).lower() for n in names}


def verb_packets(variants, src=None):
    """verb -> the play packets `Client::act` sends for it.

    Arms that send nothing directly are resolved one level through the
    `self.method(` calls they make, which is exactly how `drop`, `swap_offhand`
    and `walk` reach the wire. An arm that still resolves to nothing and is not
    in `SILENT_VERBS` is a refusal: a verb that has stopped sending is a
    coverage hole that would otherwise read as a smaller protocol.
    """
    if src is None:
        src = open(CLIENT_RS).read()
    fns = {
        m.group(1): m.start()
        for m in re.finditer(r"\n    (?:pub )?fn ([a-z_0-9]+)\s*[(<]", src)
    }
    fn_ends = sorted(fns.values()) + [len(src)]

    def fn_body(name):
        s = fns.get(name)
        if s is None:
            return ""
        return src[s : next(e for e in fn_ends if e > s)]

    # An arm starts at a `Step::` at match-arm indentation and runs to the next
    # one. Its *header* — everything up to the `=>` — can name more than one
    # variant: `Step::Dig { at, face } | Step::FinishDig { at, face } =>` is one
    # body for two verbs, and scoring only the first would report `finish_dig`
    # as a verb that sends nothing.
    marks = [(m.start(), m.group(1)) for m in re.finditer(r"\n            Step::([A-Za-z0-9]+)", src)]
    if len(marks) < 10:
        die(f"found only {len(marks)} `Step::` match arms in client.rs")
    marks.append((len(src), None))
    out = {}
    for (start, _), (end, _) in zip(marks, marks[1:]):
        arm = src[start:end]
        header = arm[: arm.find("=>")] if "=>" in arm else arm
        names = [n for n in re.findall(r"Step::([A-Za-z0-9]+)", header) if n in variants]
        if not names:
            continue
        pkts = set(PLAY_SB.findall(arm))
        for callee in set(re.findall(r"self\.([a-z_0-9]+)\s*\(", arm)):
            pkts |= set(PLAY_SB.findall(fn_body(callee)))
        for n in names:
            out.setdefault(variants[n], set()).update(pkts)
    missing = sorted(v for v in variants.values() if v not in out)
    if missing:
        die(f"variant(s) with no arm in client.rs's `act`: {', '.join(missing)}")
    empty = sorted(v for v, p in out.items() if not p and v not in SILENT_VERBS)
    if empty:
        die(f"verb(s) resolve to no packet and are not declared silent: {', '.join(empty)}")
    return out


def scenario_verbs():
    """Every `do =` name used by at least one check, and how many use it."""
    counts = {}
    n = 0
    for fn in sorted(os.listdir(CHECKS)):
        if not fn.endswith(".toml"):
            continue
        n += 1
        with open(os.path.join(CHECKS, fn), "rb") as fh:
            c = tomllib.load(fh)
        for s in c.get("setup", []) + c.get("step", []):
            counts.setdefault(s["do"], set()).add(fn)
    if n < 40:
        die(f"only {n} checks found under {CHECKS}")
    if len(counts) < 5:
        die(f"only {len(counts)} distinct verbs used across {n} checks")
    return n, counts


# --------------------------------------------------------------------------
# the server's own dispatch
# --------------------------------------------------------------------------


def _server_sources():
    out = {}
    for dirpath, _, names in os.walk(SERVER_SRC):
        for fn in names:
            if fn.endswith(".rs"):
                p = os.path.join(dirpath, fn)
                out[os.path.relpath(p, ROOT)] = strip_tests(open(p).read())
    if len(out) < 20:
        die(f"only {len(out)} .rs files under {SERVER_SRC}")
    return out


def strip_tests(src):
    """Drop `#[cfg(test)] mod tests { … }` — a test is not a dispatch arm.

    Without this, `world_send.rs`'s own unit tests hand-build frames with
    `ids::play::sb::PLAYER_INPUT` and the census would count a packet as
    dispatched because a test mentions it.
    """
    i = src.find("#[cfg(test)]")
    return src if i < 0 else src[:i]


def player_ids(src=None):
    """`crate::player::ids` — the movement decoder's own id table."""
    if src is None:
        src = open(PLAYER_IDS_RS).read()
    m = re.search(r"pub mod ids \{(.*?)\n\}", src, re.S)
    if not m:
        die("could not find `pub mod ids` in player.rs")
    names = re.findall(r"pub const ([A-Z_0-9]+)\s*:\s*i32", m.group(1))
    if len(names) < 5:
        die(f"player::ids has only {len(names)} constants")
    return set(names)


def server_dispatch(sources=None, pids=None):
    """Every serverbound play packet the server compares an incoming id against.

    Two id vocabularies reach the same wire ids — `ids::play::sb::` and
    `crate::player::ids::` — so both are matched. A name appearing only as a
    definition is not a dispatch, which is why `player.rs`'s own `pub mod ids`
    block is cut out before matching.
    """
    if sources is None:
        sources = _server_sources()
    if pids is None:
        pids = player_ids()
    hits = {}
    for path, src in sources.items():
        if path.endswith("player.rs"):
            src = re.sub(r"pub mod ids \{.*?\n\}", "", src, flags=re.S)
        for n in PLAY_SB.findall(src):
            hits.setdefault(n, set()).add(path)
        for m in re.finditer(r"\bids::([A-Z_0-9]+)\b", src):
            if m.group(1) in pids:
                hits.setdefault(m.group(1), set()).add(path)
    if len(hits) < 10:
        die(f"the server dispatches on only {len(hits)} serverbound ids")
    return hits


# The bodies a dispatch arm can reach that put bytes on a wire or move the
# shared world. Deliberately tight: over-approximating a sink moves a packet
# into `handled`, which is the flattering direction, and the whole point of
# this census is the unflattering one.
SINKS = (
    r"\.send\(\s*ids::play::cb::",
    r"crate::sounds::send\(",
    r"crate::waypoints::send\(",
    r"send_to_tracking_players",
    r"crate::broadcast::(?!tick_lock)",
    r"\.set_state\(",
    r"Entities::shared\(\)\s*\n?\s*\.update\(",
)
SINK_RE = re.compile("|".join(SINKS))


def fn_bodies(sources):
    """Every free/`impl` function in the server crate, name -> body text.

    Same-name functions in different modules are unioned, which
    over-approximates reachability. That is the wrong direction for this
    census, so it is bounded by the tightness of `SINKS` rather than by
    resolution: a name collision can only add an edge, and an added edge can
    only find a sink that some function of that name really has.
    """
    out = {}
    for src in sources.values():
        marks = [(m.start(), m.group(1)) for m in re.finditer(r"\n\s*(?:pub(?:\([a-z]+\))? )?(?:async )?fn ([a-z_0-9]+)\s*[(<]", src)]
        ends = [s for s, _ in marks[1:]] + [len(src)]
        for (s, name), e in zip(marks, ends):
            out.setdefault(name, []).append(src[s:e])
    if len(out) < 100:
        die(f"only {len(out)} functions found in the server crate")
    return out


def reaches_sink(entry, bodies, depth=6):
    """Does `entry`'s body, or anything it calls within `depth`, reach a sink?"""
    seen, frontier = set(), [entry]
    for _ in range(depth):
        nxt = []
        for name in frontier:
            if name in seen:
                continue
            seen.add(name)
            for body in bodies.get(name, ()):
                if SINK_RE.search(body):
                    return True
                nxt.extend(re.findall(r"\b([a-z_0-9]{3,})\s*\(", body))
        frontier = nxt
        if not frontier:
            break
    return False


ARM_HANDLER = re.compile(
    r"if id == ids::play::sb::([A-Z_0-9]+)(?:\s*\|\|\s*id == ids::play::sb::([A-Z_0-9]+))*\s*\{(.*?)\n                \}",
    re.S,
)


def classify(dispatched, sources, bodies):
    """handled / accepted / discarded, per dispatched packet.

    An arm whose whole body is `continue` is `discarded` — recognised on
    purpose and dropped, which `world_send.rs` does to the three chat-signing
    packets so they cannot fall through into the movement decoder.

    Everything else is put through `reaches_sink` from the function the arm
    calls. Packets decoded inside `player.rs`'s pure functions have no such
    call, so their sink is asked of the fields they write instead.
    """
    ws = sources["crates/rustmc-server/src/world_send.rs"]
    out = {}
    for m in re.finditer(
        r"if (id == ids::play::sb::[A-Z_0-9]+(?:\s*\n?\s*\|\|\s*id == ids::play::sb::[A-Z_0-9]+)*)\s*\{",
        ws,
    ):
        names = re.findall(r"sb::([A-Z_0-9]+)", m.group(1))
        body = ws[m.end() : m.end() + 400]
        body = body[: body.find("continue;") + len("continue;")] if "continue;" in body else body
        stripped = re.sub(r"//.*", "", body).strip()
        if stripped.startswith("continue;"):
            for n in names:
                out[n] = ("discarded", "arm drops the body on purpose")
            continue
        call = re.search(r"\b([a-z_0-9]{3,})\s*\(", body)
        entry = call.group(1) if call else None
        ok = bool(entry) and reaches_sink(entry, bodies)
        for n in names:
            out[n] = ("handled" if ok else "accepted", f"{entry}() -> {'sink' if ok else 'no sink'}")
    return out


def player_field_reads(sources, pids, player_src=None):
    """For each packet `player.rs` decodes, whether anything reads what it wrote.

    This is the mechanical half of the `accepted` verdict, and it is where the
    one real finding of this census came from. `apply`'s arms write named
    fields on `PlayerState`; a field written by exactly one arm and read by no
    line of this server outside `player.rs`'s own tests is a decode whose
    result is discarded, however carefully the decode itself was ported.

    Reads are counted through the two spellings this server uses for a
    player — `s.player.FIELD` and `state.FIELD` — rather than by bare field
    name, because `physics.rs` has a `horizontal_collision` of its own on a
    different struct and a bare-name grep would score `PlayerState`'s as read.
    """
    if player_src is None:
        player_src = open(PLAYER_IDS_RS).read()
    apply_fn = re.search(r"pub fn apply\(.*?\n\}", player_src, re.S)
    if not apply_fn:
        die("could not find `pub fn apply` in player.rs")
    arms = [(m.start(), m.group(1)) for m in re.finditer(r"\n        ids::([A-Z_0-9]+) =>", apply_fn.group(0))]
    if len(arms) < 4:
        die(f"player::apply has only {len(arms)} id arms")
    ends = [s for s, _ in arms[1:]] + [len(apply_fn.group(0))]
    written = {}
    for (s, name), e in zip(arms, ends):
        arm = apply_fn.group(0)[s:e]
        fields = set(re.findall(r"state\.([a-z_0-9]+)\s*=", arm))
        fields |= {
            f
            for call in re.findall(r"\b([a-z_0-9]+)\(state,", arm)
            for f in re.findall(
                r"state\.([a-z_0-9]+)\s*=",
                (re.search(rf"fn {call}\(.*?\n\}}", player_src, re.S) or re.match(r"", "")).group(0)
                if re.search(rf"fn {call}\(.*?\n\}}", player_src, re.S)
                else "",
            )
        }
        written[name] = fields
    unread = {}
    for name, fields in written.items():
        dead = set()
        for f in fields:
            reads = 0
            for path, src in sources.items():
                body = src if not path.endswith("player.rs") else re.sub(r"pub fn apply\(.*?\n\}", "", src, flags=re.S)
                body = re.sub(rf"(?:state|s\.player)\.{f}\s*=", "", body)
                reads += len(re.findall(rf"(?:state|s\.player|player)\.{f}\b", body))
            if reads == 0:
                dead.add(f)
        unread[name] = (fields, dead)
    if not any(fs for fs, _ in unread.values()):
        die("player::apply's arms were scraped as writing no fields at all")
    return unread


# --------------------------------------------------------------------------


# --------------------------------------------------------------------------
# what VANILLA does with each of the 69 — the other primary source
# --------------------------------------------------------------------------

PAIR = re.compile(
    r"(?:Game|Common|Cookie|Ping)PacketTypes\.SERVERBOUND_([A-Z_0-9]+):"
    r".*?\n\s+\d+: getstatic\s+#\d+\s+// Field ([A-Za-z0-9/$]+)\.STREAM_CODEC",
    re.S,
)
LISTENERS = (
    "net/minecraft/server/network/ServerGamePacketListenerImpl.class",
    "net/minecraft/server/network/ServerCommonPacketListenerImpl.class",
)


def resolve_handler(cls, by_class, sizes, seen=()):
    """The listener method `cls.handle` dispatches to, walking `extends`.

    Two things make this less obvious than it looks, and the second was a live
    defect in this script before its self-test covered it.

    `handle` is declared twice on most packet classes — the typed one and a
    bridge taking `PacketListener` — and the bridge's body calls the typed one,
    so a scrape that takes the first match resolves to the string `handle`.
    Every candidate is therefore filtered against the listener's own method
    table, and a class that resolves to nothing in it is a refusal, never a
    zero.

    And a **nested** packet class inherits `handle` from its outer one.
    `ServerboundMovePlayerPacket$Pos` declares none, so resolving against its
    own block alone yielded a handler of size 0 — which this script's no-op
    tier then reported as "vanilla ignores it too", for all four movement
    packets. `handleMovePlayer` is 799 instructions and the largest serverbound
    handler in the protocol. The error was silent and it was in the flattering
    direction, which is the pair of properties ORACLE rule 7 is about.
    """
    short = cls.split("/")[-1]
    if short in seen:
        return None
    b = by_class.get(short, "")
    for hm in re.finditer(r"public void handle\((?:net\.minecraft\S*)?Listener\);\s*\n\s*Code:(.*?)\n\n", b, re.S):
        for x in re.findall(r"(?:Interface)?Method [A-Za-z0-9/$]*\.?(\w+):", hm.group(1)):
            if x.startswith("handle") and x in sizes:
                return x
    m = re.search(r"\bextends (net\.minecraft\.network\.protocol\.\S+)", b)
    return resolve_handler(m.group(1).replace(".", "/"), by_class, sizes, (*seen, short)) if m else None


def vanilla_handlers(jar, disasm=None):
    """For each of the 69, the listener method vanilla runs and how big it is.

    # Why this is worth 69 javap calls

    Without it the ranked difference below can only say what *we* have. That
    ranks a packet vanilla itself throws away level with one that starts a
    structure generation, and it is the ranking somebody would work down.

    Everything here is jar-derived and none of it is guessable from the name.
    The registration lambda pairs each `SERVERBOUND_X` with the packet
    **class** — and `SERVERBOUND_BUNDLE_ITEM_SELECTED` is
    `ServerboundSelectBundleItemPacket`, so the class cannot be spelled from
    the constant. Each class's `handle(Listener)` then names the listener
    method, and the method's own disassembly gives its size.

    **A handler that is one instruction long is a `return`, and that is the
    sharp negative this function exists for**: `handlePong` and
    `handleCustomPayload` are both `0: return` in the pinned jar, so ignoring
    those two packets is *parity*, not a gap. It is the same shape as §21.2's
    provably-dead tick sections — a negative that can be checked against the
    jar rather than argued.

    Size is instruction count, and it is a rough proxy on purpose. Ten of
    those instructions are the `ensureRunningOnSameThread` preamble every
    handler carries, so the useful reading is the ordering, not the number.
    """
    import subprocess
    import tempfile
    import zipfile

    if disasm is None:
        if not os.path.exists(jar):
            die(f"pinned jar not found at {jar} — link oracle-work in (see docs/AGENT-BRIEF.md)")
        with tempfile.TemporaryDirectory() as td, zipfile.ZipFile(jar) as z:

            def dis(names):
                paths = []
                for n in names:
                    try:
                        z.extract(n, td)
                    except KeyError:
                        die(f"{n} is not in the jar — wrong jar, or the class moved")
                    paths.append(os.path.join(td, n))
                try:
                    return subprocess.run(
                        ["javap", "-p", "-c", *paths], capture_output=True, text=True, check=True
                    ).stdout
                except FileNotFoundError:
                    die("javap not on PATH — JDK 25 is required (see docs/AGENT-BRIEF.md)")
                except subprocess.CalledProcessError as e:
                    die(f"javap failed: {e.stderr.strip()[:200]}")

            gp_disasm = dis(["net/minecraft/network/protocol/game/GameProtocols.class"])
            pairs = PAIR.findall(gp_disasm)
            if len(pairs) < 60:
                die(f"only {len(pairs)} type/class pairs found in GameProtocols")
            pkt_disasm = dis([c + ".class" for _, c in pairs])
            # A nested packet class inherits `handle` from its outer one:
            # `ServerboundMovePlayerPacket$Pos` declares none, and resolving
            # against its own block alone yielded a handler of length 0 — which
            # this script's own no-op tier then reported as "vanilla ignores it
            # too", for the four movement packets. Wrong, and wrong in the
            # flattering direction. Superclasses are pulled in and walked.
            sup = {
                m.group(1).replace(".", "/")
                for m in re.finditer(r"\bextends (net\.minecraft\.network\.protocol\.\S+)", pkt_disasm)
            }
            sup -= {c for _, c in pairs}
            if sup:
                pkt_disasm += dis(sorted(c + ".class" for c in sup))
            listener_disasm = dis(list(LISTENERS))
    else:
        gp_disasm, pkt_disasm, listener_disasm = disasm
        pairs = PAIR.findall(gp_disasm)
        if len(pairs) < 60:
            die(f"only {len(pairs)} type/class pairs found in GameProtocols")

    by_class = {}
    for b in pkt_disasm.split("Compiled from ")[1:]:
        m = re.search(r"\n(?:public )?(?:final )?(?:abstract )?(?:class|record) ([A-Za-z0-9_.$]+)", b)
        if m:
            by_class[m.group(1).split(".")[-1]] = b

    sizes = {}
    ms = [(m.start(), m.group(1)) for m in re.finditer(r"\n  (?:public|private|protected)[^\n]*? (handle\w+)\(", listener_disasm)]
    if len(ms) < 40:
        die(f"only {len(ms)} handle* methods found in the listener classes")
    ends = [s for s, _ in ms[1:]] + [len(listener_disasm)]
    for (s, n), e in zip(ms, ends):
        sizes[n] = len(re.findall(r"\n\s+\d+: ", listener_disasm[s:e]))

    out = {}
    for name, cls in pairs:
        called = resolve_handler(cls, by_class, sizes)
        if called is None:
            die(
                f"{cls} resolves to no method in the listener classes — either the "
                f"packet-class scrape has gone stale or the handler has moved"
            )
        out[name] = (called, sizes[called])
    if len(out) < 60:
        die(f"resolved vanilla handlers for only {len(out)} packets")
    return out


def selftest():
    """Watch every refusal fire, in-process, with no jar and no box.

    Every extraction above returns a set, and an empty set makes this server's
    reach look like the whole protocol. The floors exist so that a scrape which
    has stopped matching refuses instead — and a guard nobody has watched fire
    is the same unverified thing as a gate that has never gone red.
    """
    import contextlib
    import io

    failures = []

    def refuses(what, fn, because):
        buf = io.StringIO()
        try:
            with contextlib.redirect_stderr(buf):
                fn()
        except SystemExit as e:
            msg = buf.getvalue()
            if e.code != 2:
                failures.append(f"{what}: exited {e.code}, expected 2")
            elif because not in msg:
                failures.append(
                    f"{what}: refused, but not for the reason under test — "
                    f"wanted {because!r}, got {msg.strip()!r}"
                )
            return
        failures.append(f"{what}: RETURNED — it should have refused")

    refuses("harness_sends: nothing matches", lambda: harness_sends("fn main() {}"), "mentions only")
    refuses(
        "harness_sends: right name, wrong protocol",
        lambda: harness_sends("\n".join(f"ids::config::sb::C{i}" for i in range(40))),
        "mentions only",
    )
    refuses("step_variants: no enum", lambda: step_variants("struct Step;"), "could not find `pub enum Step`")
    refuses(
        "step_variants: thin enum",
        lambda: step_variants("pub enum Step {\n    Hold { item: String },\n}\n"),
        "only 1 variants",
    )
    refuses("player_ids: no ids module", lambda: player_ids("fn apply() {}"), "could not find `pub mod ids`")
    refuses(
        "player_ids: thin table",
        lambda: player_ids("pub mod ids {\n    pub const A: i32 = 1;\n}"),
        "only 1 constants",
    )
    refuses("server_dispatch: empty tree", lambda: server_dispatch({}, {"X"}), "dispatches on only 0")
    refuses("fn_bodies: empty tree", lambda: fn_bodies({}), "only 0 functions")

    variants = {"Hold": "hold", "Wait": "wait", "Dig": "dig", "FinishDig": "finish_dig"}
    good = (
        "\n    fn helper(&mut self) {\n        self.conn.send(ids::play::sb::PLAYER_ACTION, &b)?;\n    }\n"
        "\n            Step::Hold { .. } => {\n                self.helper();\n                Ok(D)\n            }\n"
        "\n            Step::Dig { at } | Step::FinishDig { at } => {\n"
        "                self.conn.send(ids::play::sb::PLAYER_ACTION, &b)?;\n            }\n"
        "\n            Step::Wait { ms } => Ok(D),\n"
    ) + "".join(f"\n            Step::Pad{i} => {{}}\n" for i in range(10))
    got = verb_packets(variants, good)
    if got.get("hold") != {"PLAYER_ACTION"}:
        failures.append(f"verb_packets: did not resolve a verb through its helper — {got.get('hold')}")
    if got.get("finish_dig") != {"PLAYER_ACTION"}:
        failures.append("verb_packets: the second variant of a shared arm got no packets")
    if got.get("wait"):
        failures.append("verb_packets: a silent verb resolved to packets")
    if "Pad0" in got or "pad0" in got:
        failures.append("verb_packets: scored an arm whose variant is not in the enum")
    refuses(
        "verb_packets: a verb that sends nothing and is not declared silent",
        lambda: verb_packets({"Hold": "hold"}, good.replace("self.helper();", "")),
        "not declared silent",
    )
    refuses(
        "verb_packets: a variant the `act` match does not handle",
        lambda: verb_packets(dict(variants, Ghost="ghost"), good),
        "no arm in client.rs",
    )
    refuses("verb_packets: no arms at all", lambda: verb_packets(variants, "fn act() {}"), "found only 0")

    # The inherited-`handle` bug, which shipped and was silent. `Pos` declares
    # no `handle`; the bridge on the parent calls the typed one; only
    # `handleMovePlayer` is in the listener's table.
    parent = (
        "  public void handle(net.minecraft.network.protocol.game.ServerGamePacketListener);\n"
        "    Code:\n"
        "       2: invokeinterface #39 // InterfaceMethod X.handleMovePlayer:(LY;)V\n\n"
        "  public void handle(net.minecraft.network.PacketListener);\n"
        "    Code:\n"
        "       2: invokevirtual #40 // Method X.handle:(LZ;)V\n\n"
    )
    by_class = {
        "ServerboundMovePlayerPacket$Pos": "class ...$Pos extends net.minecraft.network.protocol.game.ServerboundMovePlayerPacket {\n",
        "ServerboundMovePlayerPacket": parent,
    }
    sizes = {"handleMovePlayer": 799}
    got = resolve_handler("a/b/ServerboundMovePlayerPacket$Pos", by_class, sizes)
    if got != "handleMovePlayer":
        failures.append(f"resolve_handler: did not follow `extends` to the declaring class — got {got!r}")
    # **The bridge is declared FIRST here, and that is the whole fixture.**
    # With the typed method first, dropping the `x in sizes` filter still
    # returns the right answer and the mutation survives — measured. A fixture
    # whose declaration order does not punish taking the first match cannot
    # tell the two implementations apart.
    bridge_first = dict(
        by_class,
        ServerboundMovePlayerPacket=(
            "  public void handle(net.minecraft.network.PacketListener);\n"
            "    Code:\n"
            "       2: invokevirtual #40 // Method X.handle:(LZ;)V\n\n"
        )
        + parent,
    )
    if resolve_handler("a/b/ServerboundMovePlayerPacket", bridge_first, sizes) != "handleMovePlayer":
        failures.append("resolve_handler: took the bridge method's `handle` over the typed one")
    if resolve_handler("a/b/ServerboundMovePlayerPacket$Pos", bridge_first, sizes) != "handleMovePlayer":
        failures.append("resolve_handler: bridge-first, inherited — took `handle` over the typed one")
    if resolve_handler("a/b/Nope", by_class, sizes) is not None:
        failures.append("resolve_handler: invented a handler for a class it has never seen")
    loop = {"A": "class A extends net.minecraft.network.protocol.B {\n", "B": "class B extends net.minecraft.network.protocol.A {\n"}
    if resolve_handler("A", loop, sizes) is not None:
        failures.append("resolve_handler: an `extends` cycle did not terminate at None")

    if strip_tests("live\n#[cfg(test)]\nmod tests { ids::play::sb::GHOST }") != "live\n":
        failures.append("strip_tests: did not cut the test module")

    graph = {"outer": ["inner(); "], "inner": ["self.conn.send(ids::play::cb::X, &v)"]}
    if reaches_sink("outer", graph) is not True:
        failures.append("reaches_sink: missed a sink one call deep")
    if reaches_sink("outer", {"outer": ["inner(); "], "inner": ["nothing_here()"]}) is not False:
        failures.append("reaches_sink: invented a sink")
    if reaches_sink("outer", {"outer": ["crate::broadcast::tick_lock();"]}) is not False:
        failures.append("reaches_sink: tick_lock counted as a sink — it is a mutex, not a wire")
    if reaches_sink("outer", graph, depth=1) is not False:
        failures.append("reaches_sink: depth bound is not enforced")

    for f in failures:
        print(f"serverbound-surface selftest: FAIL — {f}", file=sys.stderr)
    if failures:
        return 1
    print("serverbound-surface selftest: OK — 11 refusals fired for their own reason, 11 cross-checks agreed")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jar", default=None)
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        sys.exit(selftest())

    cs = _corpus_surface()
    jar = args.jar or cs.JAR
    jar_cb, jar_sb = cs.jar_protocol(jar)
    our_cb, our_sb = cs.ids_rs()
    # The fatal cross-check `ci/corpus-surface.py` runs, run here too: without
    # it, registration order is not id order and every row below names the
    # wrong packet.
    drift = cs.proto_drift(jar_cb, jar_sb, our_cb, our_sb)
    if drift:
        print("serverbound-surface: REFUSED — rustmc-proto disagrees with the pinned jar:", file=sys.stderr)
        for d in drift:
            print(f"  {d}", file=sys.stderr)
        sys.exit(2)

    sendable = harness_sends() & set(jar_sb)
    variants = step_variants()
    vp = verb_packets(variants)
    n_checks, used = scenario_verbs()
    sources = _server_sources()
    pids = player_ids()
    dispatched = server_dispatch(sources, pids)
    bodies = fn_bodies(sources)
    vanilla = vanilla_handlers(jar)
    missing = [n for n in jar_sb if n not in vanilla]
    if missing:
        die(f"no vanilla handler resolved for {', '.join(missing)}")
    verdicts = classify(dispatched, sources, bodies)
    unread = player_field_reads(sources, pids)

    # Packets `player.rs` decodes are classified by whether anything reads the
    # fields their arm wrote.
    for name, (fields, dead) in unread.items():
        if not fields:
            continue
        verdicts[name] = (
            ("accepted", f"writes {', '.join(sorted(dead))}, read nowhere")
            if dead == fields
            else ("handled", f"writes {', '.join(sorted(fields - dead))}, read downstream")
        )
    for name in dispatched:
        if name not in verdicts:
            # `apply_hand` and `input_shift` are called by value rather than
            # matched by id in the loop, so they have no `if id ==` arm.
            verdicts[name] = ("handled", "decoded by a called predicate that the loop acts on")

    for n, (v, _) in verdicts.items():
        if v == "accepted" and n not in ACCEPTED_REASON:
            die(
                f"{n} decodes into nothing any line of this server reads, and there is no "
                f"entry for it in ACCEPTED_REASON — write down why, or give it a reader"
            )
    for n in ACCEPTED_REASON:
        if verdicts.get(n, ("",))[0] != "accepted":
            die(f"ACCEPTED_REASON names {n}, which the analysis does not find in that state — drop the entry")

    # Which packets a scenario's own steps put on the wire, as opposed to the
    # ones the connection machinery sends in every run regardless.
    by_steps = set()
    for verb in used:
        by_steps |= vp.get(verb, set())
    connection = sendable - {p for pkts in vp.values() for p in pkts}
    sent = by_steps | connection

    handled = {n for n, (v, _) in verdicts.items() if v == "handled"}
    accepted = {n for n, (v, _) in verdicts.items() if v == "accepted"}
    discarded = {n for n, (v, _) in verdicts.items() if v == "discarded"}
    named = {n for n in jar_sb if n in our_sb}

    print(f"pinned jar   {os.path.relpath(jar, ROOT)}")
    print(f"corpus       {n_checks} checks, {len(used)} of {len(variants)} verbs used")
    print("recordings   none exist — trace::observe runs on the read path only,")
    print("             so no serverbound packet is recorded by anything, ever")
    print()
    print("== serverbound play surface ==")
    print(f"  {len(jar_sb):3d}  in the protocol            (GameProtocols, pinned jar)")
    print(f"  {len(named):3d}  named in rustmc-proto      (an id constant exists)")
    print(f"  {len(sendable):3d}  the harness can send       (ids::play::sb in client.rs)")
    print(f"  {len(sent):3d}  at least one scenario sends")
    print(f"       {len(by_steps):3d}  by a scenario's own steps")
    print(f"       {len(connection):3d}  by the connection machinery, in every run")
    print(f"  {len(dispatched):3d}  the server has an arm for")
    print(f"       {len(handled):3d}  HANDLED    the arm reaches an observable sink")
    print(f"       {len(accepted):3d}  ACCEPTED   decoded, and nothing reads the result")
    print(f"       {len(discarded):3d}  DISCARDED  the arm drops the body on purpose")
    print()
    visible = handled & sent
    print(f"  {len(visible):3d}  handled AND sent  <- the most a check could possibly fail on")
    print(f"  {len(jar_sb) - len(visible):3d}  in the state where no check in this repository can fail")
    print()

    if not args.quiet:
        print("== the ranked set difference ==")
        print("   sendable/sent/arm are ours; the rank is what closing it would cost")
        print("   the last column is vanilla's own handler and its instruction count")
        print()
        print("   Tier 1 is NOT 'a check would fail today'. Whether a gap is observable")
        print("   depends on what vanilla emits in response and on whether the recorder")
        print("   keeps it: trace::FAMILIES makes chunk_batch_start/finished ambient")
        print("   before any decoder, so CHUNK_BATCH_RECEIVED's whole clientbound effect")
        print("   is invisible by construction. docs/PARITY-ROADMAP.md §40.")
        rank = []
        for i, n in enumerate(jar_sb):
            cols = (
                "N" if n in named else ".",
                "S" if n in sendable else ".",
                "s" if n in sent else ".",
                "A" if n in dispatched else ".",
            )
            v = verdicts.get(n, ("none", "no arm anywhere in the server"))
            method, size = vanilla[n]
            if n in visible:
                cost = "0 covered"
            elif size <= 1:
                cost = "0 vanilla ignores it too — parity, not a gap"
            elif n in sent and n not in dispatched:
                cost = "1 sent into a server that has no arm for it"
            elif n in accepted:
                cost = "1 decoded and discarded — an arm with no reader"
            elif n in discarded:
                cost = "2 recognised and dropped on purpose"
            elif n in sendable:
                cost = "3 sendable, no scenario sends it"
            elif n in dispatched:
                cost = "3 an arm exists, the harness cannot send it"
            elif n in named:
                cost = "4 id constant only"
            else:
                cost = "5 absent from this tree entirely"
            rank.append((cost, -size, i, n, "".join(cols), method, size, v[1]))
        for cost, _, i, n, cols, method, size, why in sorted(rank):
            print(f"  {cost[0]}  0x{i:02X} {n:30s} [{cols}]  {cost[2:]:44s} {method}/{size}")
        print()
        noop = [n for n in jar_sb if vanilla[n][1] <= 1]
        print("== the checkable negative: vanilla's own handler is a bare return ==")
        print("   (ignoring one of these is parity. It is the only tier below whose")
        print("    zero cost is read out of the jar rather than out of our own tree.)")
        for n in noop:
            print(f"  0x{jar_sb.index(n):02X} {n:30s} {vanilla[n][0]}  {vanilla[n][1]} instruction")
        print()

        print("== the verbs ==")
        for verb in sorted(set(variants.values())):
            pk = ", ".join(sorted(vp.get(verb, ()))) or "(sends nothing)"
            n = len(used.get(verb, ()))
            print(f"  {verb:16s} {n:3d} checks  {pk}")
        print()

        print("== what each decoded packet's arm actually does ==")
        for n in sorted(verdicts, key=lambda n: jar_sb.index(n) if n in jar_sb else 99):
            v, why = verdicts[n]
            print(f"  {n:30s} {v:10s} {why}")
        print()

    print("== reproducibility, in place of a floor ==")
    print("  A floor is the disagreement between two innocent runs of one thing, and")
    print("  there is nothing here for two runs to disagree about: every set above is")
    print("  read out of the pinned jar or out of this tree, so ORACLE rule 6 has no")
    print("  premise to fire on and this census carries NO floor. What stands in its")
    print("  place: the script is a pure function of the jar and the tree, so two runs")
    print("  at one commit are byte-identical and the spread is exactly 0 — which is a")
    print("  statement about determinism, NOT about resolving power. It has none.")
    print()
    print("== the headline ==")
    noop = {n for n in jar_sb if vanilla[n][1] <= 1}
    blind = [n for n in jar_sb if n not in visible]
    print(f"  {len(visible)} of {len(jar_sb)} serverbound play packet types are both handled by this")
    print(f"  server and sent by a scenario. The other {len(blind)} cannot make any check in")
    print("  this repository go red, because a packet we ignore changes nothing we emit.")
    print(f"  {len(noop & set(blind))} of those {len(blind)} are ones vanilla ignores too — its own handler is a")
    print(f"  bare return — so the honest size of the blind spot is {len(blind) - len(noop & set(blind))}.")


if __name__ == "__main__":
    main()
