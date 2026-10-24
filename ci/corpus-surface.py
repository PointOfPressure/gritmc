#!/usr/bin/env python3
"""corpus-surface.py — how much of vanilla's play protocol the corpus can see at all.

    ./ci/corpus-surface.py
    ./ci/corpus-surface.py --recordings '~/rustmc*/oracle-work/gameplay/vanilla.json'

`ci/corpus-coverage.py` answers the *inside* question: of the things this
harness already knows how to say and hear, how many checks use each. It cannot
answer the outside one, because every set it counts is read out of our own
sources — so a mechanism vanilla has and this harness has never heard of is
invisible to it, and 100% of a small set reads exactly like 100% of a large
one.

This script supplies the denominator. It reads the **pinned jar** for the two
things a play connection can carry — the 141 clientbound and 69 serverbound
packet types `GameProtocols` registers, in id order — and prints the set
difference against four progressively narrower sets of our own:

    jar          every packet type in the protocol             (the surface)
    named        we have an id constant for it                 (rustmc-proto)
    legible      the recorder turns it into something           (trace.rs)
                 other than an opaque digest
    observed     at least one check projects it                (ci/checks)

and, when recordings are available, a fifth that is a **measurement** rather
than a reading:

    emitted      vanilla actually sent it during a corpus run

That last column is the one that matters, and it is why this script takes
recordings. Two thirds of the clientbound surface is unreachable from a
creative superflat with two players and no mobs — a coverage report that
counts advancement, boss-bar, map and resource-pack packets as gaps is
arithmetic, not information. What vanilla *did* send while the 88 ran is not
a judgement about reachability; it is the reachable set, sampled.

# Three tiers of blindness, and they need different fixes

* **ambient** — `trace::FAMILIES` classifies the packet `Class::Ambient`, so
  it is recorded and then dropped by `compare::project` before any
  decode. Nothing downstream can ever see it. Twelve ids.
* **opaque** — no decoder arm, so the packet is recorded as `packet:0xNN`
  with its whole body digested into `rest`. This is *not* invisible: a check
  that names the kind compares every byte. It is the cheapest coverage in the
  corpus, because the recording already exists.
* **unobserved** — legible, and no check names it. A decoder arm nobody
  projects is dead weight that reads as coverage.

# The extraction can fail, and it says so rather than reporting a zero

Same discipline as `ci/corpus-coverage.py`, for the same reason: every set
here is scraped, a scrape that stops matching returns the empty set, and an
empty set makes the coverage look total. Each extraction asserts a floor and
this script exits 2 rather than print a number it cannot stand behind.

The jar extraction has a stronger check than a floor. Every packet id it
derives is cross-checked against our own `ids::play` constants, and any
disagreement is fatal — so this script is also a drift detector for
`rustmc-proto`. It found none on 2026-08-10: all 141 clientbound and 69
serverbound names we carry a constant for agree with the jar on the number.
"""

import argparse
import glob
import json
import os
import re
import subprocess
import sys
import tempfile
import tomllib
import zipfile

from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHECKS = os.path.join(ROOT, "ci", "checks", "gameplay")
JAR = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "versions", "26.2", "server-26.2.jar")
PROTOCOLS = "net/minecraft/network/protocol/game/GameProtocols.class"

# `GameProtocols`' static initialiser builds the two protocols with one lambda
# each. Which lambda is which is not guessable from the name — `$1` is
# serverbound and `$2` is clientbound in the pinned jar — so it is decided by
# what the body references rather than by its number.
LAMBDA = re.compile(r"^  private static void (lambda\$static\$\d+)\(", re.M)
PKT = re.compile(r"(?:Game|Common|Cookie|Ping)PacketTypes\.([A-Z_0-9]+)")

# Places where we call a packet something other than what the jar calls it.
# Declared rather than tolerated: the id cross-check below is fatal on any
# other mismatch, and an undeclared rename is indistinguishable from an id
# that has moved. `ours -> jar's`.
ALIASES = {
    # `ClientboundBundlePacket` is the delimiter itself — vanilla's type is
    # named for the packet, ours for what it does. Same id 0, same wire shape
    # (empty body).
    "BUNDLE_DELIMITER": "BUNDLE",
}


def die(msg):
    print(f"corpus-surface: REFUSED — {msg}", file=sys.stderr)
    sys.exit(2)


# --------------------------------------------------------------------------
# the surface: the pinned jar
# --------------------------------------------------------------------------


def jar_protocol(jar):
    """The clientbound and serverbound play packet names, in id order.

    Registration order in `GameProtocols` **is** the id order — `ProtocolInfoBuilder`
    hands out consecutive ids as `addPacket` is called. Verified against our
    own constants by the caller rather than asserted here.
    """
    if not os.path.exists(jar):
        die(f"pinned jar not found at {jar} — link oracle-work in (see docs/AGENT-BRIEF.md)")
    with tempfile.TemporaryDirectory() as td:
        with zipfile.ZipFile(jar) as z:
            try:
                z.extract(PROTOCOLS, td)
            except KeyError:
                die(f"{PROTOCOLS} is not in {jar} — wrong jar, or the class moved")
        try:
            out = subprocess.run(
                ["javap", "-p", "-c", "-constants", os.path.join(td, PROTOCOLS)],
                capture_output=True,
                text=True,
                check=True,
            ).stdout
        except FileNotFoundError:
            die("javap not on PATH — JDK 25 is required (see docs/AGENT-BRIEF.md)")
        except subprocess.CalledProcessError as e:
            die(f"javap failed on GameProtocols: {e.stderr.strip()[:200]}")

    return parse_protocol(out)


def parse_protocol(out):
    """The pure half of `jar_protocol`, so that its refusals can be tested.

    Split out for `--selftest`: every `die` below is a staleness guard, and a
    guard nobody has watched fire is the same unverified thing as a gate that
    has never gone red.
    """
    # Split the disassembly at each method header, then attribute each body by
    # the direction of the packet names it mentions. A body that mixes the two
    # would mean the registration shape has changed and every id below would be
    # wrong, so that is fatal rather than filtered.
    marks = [(m.start(), m.group(1)) for m in LAMBDA.finditer(out)] + [(len(out), None)]
    cb, sb = None, None
    for (start, name), (end, _) in zip(marks, marks[1:]):
        names = PKT.findall(out[start:end])
        if not names:
            continue
        c = [n for n in names if n.startswith("CLIENTBOUND_")]
        s = [n for n in names if n.startswith("SERVERBOUND_")]
        if c and s:
            die(f"{name} registers both directions — GameProtocols' shape has changed")
        if c:
            if cb is not None:
                die("two clientbound registration lambdas found")
            cb = c
        elif s:
            if sb is not None:
                die("two serverbound registration lambdas found")
            sb = s
    if cb is None or sb is None:
        die("could not find both registration lambdas in GameProtocols")
    for direction, lst in (("clientbound", cb), ("serverbound", sb)):
        dup = [n for n, k in Counter(lst).items() if k > 1]
        if dup:
            die(f"{direction} packet registered twice: {', '.join(dup)}")
    if len(cb) < 100 or len(sb) < 50:
        die(f"extracted only {len(cb)} clientbound and {len(sb)} serverbound packet types")
    strip = lambda ns, p: [n[len(p) :] for n in ns]  # noqa: E731
    return strip(cb, "CLIENTBOUND_"), strip(sb, "SERVERBOUND_")


# --------------------------------------------------------------------------
# our own sets
# --------------------------------------------------------------------------


def ids_rs(src=None):
    """`play::cb` and `play::sb` constants, as name -> id."""
    if src is None:
        src = open(os.path.join(ROOT, "crates", "rustmc-proto", "src", "ids.rs")).read()
    play = re.search(r"pub mod play \{(.*?)\n\}", src, re.S)
    if not play:
        die("could not find `pub mod play` in ids.rs")
    out = {}
    for d in ("sb", "cb"):
        body = re.search(rf"pub mod {d} \{{(.*?)\n    \}}", play.group(1), re.S)
        if not body:
            die(f"could not find `pub mod {d}` inside `pub mod play` in ids.rs")
        out[d] = {
            n: int(v, 0)
            for n, v in re.findall(
                r"pub const ([A-Z_0-9]+)\s*:\s*i32\s*=\s*(0x[0-9A-Fa-f]+|\d+)", body.group(1)
            )
        }
    # The floors are deliberately well under the true counts (57 cb / 27 sb on
    # 2026-08-10). They exist to catch a regex that has stopped matching, not
    # to assert coverage — this server names less than half the protocol and
    # that is a finding this script reports, not an error it refuses on.
    if len(out["cb"]) < 40 or len(out["sb"]) < 18:
        die(f"extracted only {len(out['cb'])} cb and {len(out['sb'])} sb constants from ids.rs")
    return out["cb"], out["sb"]


def ambient_ids(src):
    """The clientbound ids `trace::FAMILIES` classifies as `Class::Ambient`.

    # THIS USED TO PARSE `fn is_housekeeping` AND THAT FUNCTION IS GONE

    Until 2026-09-06 the recorder dropped twelve packet types at **record**
    time, and this regex read that function's `cb::NAME` mentions. Nothing is
    dropped at record time now: the classification is one declared table with
    a `class` and a stated `reason` per row, and the drop happens in
    `compare::project`. So what a ceiling below "spends" is no longer a packet
    being thrown away — it is a packet being **ambient by default**, which is
    the same premise for every ceiling here (an ambient kind reaches a
    projection only if a scenario names it, and no scenario in the corpus
    does) and is a weaker claim honestly stated rather than a stronger one
    silently retired.

    Refuses the two ways this can go stale: a table it cannot find, and a
    table so small the regex has plainly stopped matching rows.
    """
    tbl = re.search(r"pub const FAMILIES: &\[Family\] = &\[.*?\n\];", src, re.S)
    if not tbl:
        die("could not find `pub const FAMILIES` in trace.rs")
    rows = re.findall(
        r"Family\s*\{(.*?)\n    \}", tbl.group(0), re.S
    )
    if len(rows) < 8:
        die(f"FAMILIES matched only {len(rows)} rows")
    keep = set()
    for body in rows:
        if "Class::Ambient" not in body:
            continue
        m = re.search(r"id:\s*Some\(ids::play::cb::([A-Z_0-9]+)\)", body)
        if m:
            keep.add(m.group(1))
    if len(keep) < 8:
        die(f"FAMILIES declares only {len(keep)} ambient clientbound ids")
    return keep


def undecoded_families(src=None):
    """Classified families for which `trace::decode` has **no arm**.

    The distinction `ambient_ids` cannot make and a ceiling below depends on.
    It was written for `LEVEL_CHUNK_WITH_LIGHT`, which was ambient *and*
    undecoded until the chunk block-entity arm landed on main
    (`trace-chunk-block-entities`); that packet now has an arm and no
    `FAMILIES` row, so no classified family currently spends this set.
    """
    if src is None:
        src = open(os.path.join(ROOT, "tools", "oracle-runner", "src", "trace.rs")).read()
    tbl = re.search(r"pub const FAMILIES: &\[Family\] = &\[.*?\n\];", src, re.S)
    if not tbl:
        die("could not find `pub const FAMILIES` in trace.rs")
    out = set()
    for body in re.findall(r"Family\s*\{(.*?)\n    \}", tbl.group(0), re.S):
        m = re.search(r"id:\s*Some\(ids::play::cb::([A-Z_0-9]+)\)", body)
        if m and re.search(r"arm:\s*false", body):
            out.add(m.group(1))
    return out


def trace_sets(src=None):
    """What `trace.rs` drops, and what it decodes — as `cb::NAME` -> kind."""
    if src is None:
        src = open(os.path.join(ROOT, "tools", "oracle-runner", "src", "trace.rs")).read()

    keep = ambient_ids(src)

    dec = re.search(r"^fn decode\(.*?\n^\}", src, re.S | re.M)
    if not dec:
        die("could not find `fn decode` in trace.rs")
    # Each arm is `x if x == cb::A [|| x == cb::B] => { ... "kind".into() }`.
    # The kind is the last `"...".into()` before the arm closes, and arms are
    # separated by the next guard, so slicing on the guards is exact enough and
    # far more robust than trying to balance braces.
    guards = [
        (m.start(), re.findall(r"cb::([A-Z_0-9]+)", m.group(0)))
        for m in re.finditer(r"x if x == cb::[A-Z_0-9]+(?:\s*\|\|\s*x == cb::[A-Z_0-9]+)*\s*=>", src[dec.start() : dec.end()])
    ]
    base = dec.start()
    guards = [(base + s, n) for s, n in guards] + [(dec.end(), None)]
    out = {}
    for (start, names), (end, _) in zip(guards, guards[1:]):
        body = src[start:end]
        # ignore the field-name arguments, exactly as corpus-coverage.py does
        body = re.sub(r'insert\(\s*"[a-z_]+"\.into\(\)', "insert(", body)
        lits = re.findall(r'"([a-z_]+)"\.into\(\)', body)
        consts = re.findall(r"\b([A-Z_][A-Z0-9_]*)\.into\(\)", body)
        kind = lits[-1] if lits else None
        if kind is None and consts:
            cmap = dict(
                re.findall(r'const\s+([A-Z_][A-Z0-9_]*)\s*:\s*&str\s*=\s*"([a-z_]+)"', src)
            )
            kind = cmap.get(consts[-1])
        for n in names:
            out[n] = kind
    if len(out) < 30:
        die(f"decode() yielded only {len(out)} arms")
    if any(v is None for v in out.values()):
        die("a decode arm yielded no kind name — the arm-slicing regex has gone stale")
    return keep, out


def client_sends(src=None):
    """Serverbound packet names `client.rs` is able to put on the wire."""
    if src is None:
        src = open(os.path.join(ROOT, "tools", "oracle-runner", "src", "client.rs")).read()
    names = set(re.findall(r"sb::([A-Z_0-9]+)", src))
    if len(names) < 15:
        die(f"client.rs mentions only {len(names)} serverbound ids")
    return names


def proto_drift(jar_cb, jar_sb, our_cb, our_sb):
    """Every id we name, against the jar's registration order.

    Registration order is only the id order if our own constants agree, and if
    they do not then every row this script prints is attributed to the wrong
    packet. Returned rather than printed so `--selftest` can perturb an id and
    watch this fire.
    """
    drift = []
    for direction, jar_list, ours in (("cb", jar_cb, our_cb), ("sb", jar_sb, our_sb)):
        by_id = {i: n for i, n in enumerate(jar_list)}
        for name, i in sorted(ours.items(), key=lambda kv: kv[1]):
            if by_id.get(i) != ALIASES.get(name, name):
                drift.append(f"play::{direction}::{name} = {i} (0x{i:02X}), jar has {by_id.get(i, '<none>')}")
    return drift


def load_checks():
    out = {}
    for fn in sorted(os.listdir(CHECKS)):
        if fn.endswith(".toml"):
            with open(os.path.join(CHECKS, fn), "rb") as fh:
                out[fn] = tomllib.load(fh)
    if not out:
        die(f"no checks found under {CHECKS}")
    return out


BLOCK_PKG = "net/minecraft/world/level/block/"


class Hierarchy:
    """Resolve a block class to the class that actually **declares** a method.

    Without this the class column is wrong in the direction that invents work.
    `use_without_item.json` records each block's *runtime* class, so
    `TrappedChestBlock`, `CopperChestBlock` and `WeatheringCopperChestBlock`
    are three rows — but none of them declares `useWithoutItem`; all three
    inherit `ChestBlock`'s, which check `090` already covers. Counting runtime
    classes reported 12 of 47 covered on 2026-08-10; counting implementations
    reports **11 of 39**, and the eight that vanish are inheritance, not
    coverage. The naive number would have sent somebody to write three chest
    checks that exercise one method.

    javap is invoked once per class and memoised. Absent jar, absent answer —
    the caller falls back to the runtime column and says which it printed.
    """

    def __init__(self, jar):
        self.ok = os.path.exists(jar)
        self.z = zipfile.ZipFile(jar) if self.ok else None
        self.paths = (
            {n.rsplit("/", 1)[1][:-6]: n for n in self.z.namelist() if n.startswith(BLOCK_PKG) and n.endswith(".class")}
            if self.ok
            else {}
        )
        self.td = tempfile.mkdtemp(prefix="corpus-surface-") if self.ok else None
        self._info = {}

    def _read(self, cls):
        if cls in self._info:
            return self._info[cls]
        p = self.paths.get(cls)
        if not p:
            self._info[cls] = (set(), None)
            return self._info[cls]
        self.z.extract(p, self.td)
        try:
            out = subprocess.run(
                ["javap", "-p", os.path.join(self.td, p)], capture_output=True, text=True, check=True
            ).stdout
        except (OSError, subprocess.CalledProcessError):
            self._info[cls] = (set(), None)
            return self._info[cls]
        methods = set(re.findall(r"\b([a-zA-Z][A-Za-z0-9_]*)\(", out))
        m = re.search(r"\bextends ([A-Za-z0-9_.$]+)", out)
        self._info[cls] = (methods, m.group(1).rsplit(".", 1)[-1] if m else None)
        return self._info[cls]

    def declaring(self, cls, method):
        seen = set()
        while cls and cls not in seen:
            seen.add(cls)
            methods, sup = self._read(cls)
            if method in methods:
                return cls
            cls = sup
        return None


# Which method each behaviour table is a table *of*, so the class column can be
# collapsed onto implementations. A table not named here keeps its runtime
# classes and says so.
TABLE_METHOD = {
    "use_without_item": "useWithoutItem",
    "set_placed_by": "setPlacedBy",
    # Added 2026-08-12 with `data/vanilla/use_item_on.json` (§23). Without the
    # mapping this table still scores, but on its 25 **runtime** classes with
    # the "not collapsed" tag — and the two right-click entry points would then
    # be quoted in two different units under one heading.
    "use_item_on": "useItemOn",
}


def placeable_blocks(held, vanilla_dir):
    """Held **item** ids -> the **block** ids those items place.

    The two are different namespaces and this comparison was `b in held`, a
    string test between a block id and a set of item ids, until 2026-08-12. It
    is right whenever an item and its block share a name and wrong whenever
    they do not: the corpus holds `minecraft:redstone`, eight checks place
    redstone dust with it, and `use_without_item.json`'s
    `minecraft:redstone_wire` row scored uncovered through every one of them.
    §21.5's "16 of 184 blocks, 14 of 39 classes" was one short in both units.

    `data/vanilla/block_items.json` (`ci/block-items-golden.sh`) reads
    `BlockItem.getBlock()` off the pinned jar for all 1,537 items; 1,054 place
    a block and **14 place one under a different id** — the crops, cocoa,
    tripwire from string, powder snow from a bucket, cave vines from glow
    berries, and redstone.

    The item ids are kept in the returned set as well as the blocks they place.
    A block whose id happens to match an item id it is unrelated to would then
    still score, which is the pre-existing behaviour and cannot be worse than
    it; dropping them would silently change every figure this function has ever
    produced for a reason that has nothing to do with the defect being fixed.

    Absent table: return `held` unchanged. This is a refinement of a bound, not
    a precondition — refusing here would take out the protocol axis too, which
    does not use it.
    """
    p = os.path.join(vanilla_dir, "block_items.json")
    try:
        with open(p) as f:
            m = json.load(f).get("item_to_block", {})
    except (OSError, json.JSONDecodeError):
        return held
    return set(held) | {m[i] for i in held if i in m}


def behaviour_tables(held, hier=None):
    """Block-behaviour surface, from the tables the dumpers derive from the jar.

    The protocol axis above bounds what the corpus can *hear*. This one bounds
    what it can *touch*, and the two fail differently: a corpus can observe
    every packet vanilla sends and still exercise one block class in forty.

    The corpus has no verb that places a block directly — the world is built
    entirely by `hold` then `place` — so **the set of items it holds is the set
    of blocks it can involve at all**, and every behaviour table is scored
    against that set. This is an upper bound on coverage, not a claim of it: a
    block held once and placed once has had one of its states exercised.

    Class coverage is the honest column. One class is one implementation, so
    covering `minecraft:oak_door` covers `DoorBlock` for `minecraft:iron_door`
    too, and the per-block column would report that as a 1-in-N hole it is not.

    Tables key their rows either by `block` or by a `state` string whose prefix
    is the block; both shapes are handled, and a table with neither is skipped
    with its name printed rather than silently scored zero.

    **`class` must be a string, and that is not pedantry.** The first cut of
    this function scored every table carrying a `class` key, and
    `update_shape.json`'s `class` is an **integer shape-group index**, not a
    class name — 139 of its blocks map to more than one, `minecraft:piston_head`
    to twelve. It duly reported "22 of 118 classes covered" from 15 covered
    blocks, a ratio with no referent, in a section whose whole purpose is to be
    quoted. Tables whose `class` is not a name are listed as skipped rather
    than scored.
    """
    d = os.path.join(ROOT, "data", "vanilla")
    if not os.path.isdir(d):
        return None
    held = placeable_blocks(held, d)
    out, skipped = [], []
    for fn in sorted(os.listdir(d)):
        if not fn.endswith(".json"):
            continue
        try:
            tbl = json.load(open(os.path.join(d, fn)))
        except (OSError, json.JSONDecodeError):
            continue
        tbl = tbl.get("table", tbl) if isinstance(tbl, dict) else tbl
        if not isinstance(tbl, list) or not tbl or not isinstance(tbl[0], dict):
            continue
        if "class" not in tbl[0]:
            continue
        if not isinstance(tbl[0]["class"], str):
            skipped.append((fn[:-5], type(tbl[0]["class"]).__name__))
            continue
        method = TABLE_METHOD.get(fn[:-5])
        resolve = hier and hier.ok and method
        blocks, classes, hit_b, hit_c = set(), set(), set(), set()
        for e in tbl:
            b = e.get("block") or (e.get("state", "").split("[")[0] or None)
            c = e.get("class")
            if not b or not c:
                continue
            if resolve:
                c = hier.declaring(c, method) or c
            blocks.add(b)
            classes.add(c)
            if b in held:
                hit_b.add(b)
                hit_c.add(c)
        if blocks:
            out.append((fn[:-5], len(blocks), len(hit_b), len(classes), len(hit_c), bool(resolve)))
    return out, skipped


def recordings_census(pattern):
    """Kinds vanilla actually emitted, counted over every recording matched.

    Returns `(files, Counter(kind -> occurrences), {kind -> set(scenario)})`.
    An absent or empty match is not an error — the reading is simply omitted,
    and the caller says so rather than printing an unmarked zero.
    """
    files = sorted(glob.glob(os.path.expanduser(pattern)))
    tot, where = Counter(), defaultdict(set)
    # per[kind][scenario] = list of per-run occurrence counts, one entry per
    # run that produced the kind at all. Absence from a run is not a zero in
    # this list — it is a shorter list, and the caller reads that as the
    # sharper instability, because a kind that appears in 9 runs of 17 is a
    # worse floor than one whose count merely wobbles.
    per = defaultdict(lambda: defaultdict(list))
    for p in files:
        try:
            d = json.load(open(p))
        except (OSError, json.JSONDecodeError):
            continue
        for s in d.get("scenarios", []):
            c = Counter(o["kind"] for o in s.get("obs", []) if o.get("kind"))
            for k, n in c.items():
                tot[k] += n
                where[k].add(s.get("name", "?"))
                per[k][s.get("name", "?")].append(n)
    return files, tot, where, per


def stability(files, per, kinds):
    """Is a kind's occurrence count reproducible, per scenario, across runs?

    This is the cheap pre-screen the corpus did not have. A new check must
    produce a stable reading across independent vanilla runs or be declared
    unstable with its rate — and the usual way to establish that is to record
    the corpus N times, which is hours. But every tree that has ever run
    `ci/t2-gameplay.sh` left a vanilla recording behind, and a kind that is
    already being emitted inside a window the corpus records has therefore
    **already been sampled N times**. Screening a candidate projection against
    those recordings costs seconds and answers the question before a check
    exists to ask it.

    It is a pre-screen and not a substitute: it can only speak about rigs that
    already exist. A brand new rig still has to be recorded. What it does
    settle, for free, is whether the *kind* is reproducible in a rig of that
    shape — and it has already ruled one candidate out. `rotate_head` is
    perfectly stable in three scenarios and swings 2..17 in a fourth, so
    "observe rotate_head" is not a property of the kind at all; it is a
    property of how much the rig moves.
    """
    rows = []
    for k in kinds:
        for name, counts in sorted(per.get(k, {}).items()):
            uniq = sorted(set(counts))
            if len(counts) < len(files):
                verdict = f"present in {len(counts)} of {len(files)} runs"
            elif len(uniq) > 1:
                verdict = f"count varies {uniq}"
            else:
                verdict = f"stable at {uniq[0]}"
            rows.append((k, name, len(counts), verdict))
    return rows


# --------------------------------------------------------------------------


def selftest():
    """Watch every refusal fire. A guard nobody has seen fire is not a guard.

    Each extraction in this file returns a set, an empty set makes coverage
    look total, and the floors and cross-checks exist so that a scrape which
    has stopped matching refuses instead. `ci/t0.sh` runs
    `ci/admissible-residual.py --selftest` for the same reason and this is the
    same contract: the guards are exercised against deliberately broken input,
    in-process, with no jar and no box.
    """
    import contextlib
    import io

    failures = []

    def refuses(what, fn, because):
        """Assert not just that it refused, but that the named guard is what refused.

        **The first version of this took no `because` and was worthless.**
        Every guard in `parse_protocol` exits 2, and the floor guard runs last,
        so a fixture built to trip the duplicate-packet guard trips the floor
        instead once it is only one packet long. Deleting the duplicate check
        outright — `dup = []` — left this selftest reporting OK, which is the
        exact shape of the thing it exists to prevent. Verified by doing it:
        broken guard, selftest green. The message is now the assertion.
        """
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

    def accepts(what, fn):
        try:
            return fn()
        except SystemExit:
            failures.append(f"{what}: refused valid input")
            return None

    # --- parse_protocol -----------------------------------------------------
    good = (
        "  private static void lambda$static$1(x);\n"
        "    Code:\n"
        "       0: getstatic  // Field GamePacketTypes.SERVERBOUND_A\n"
        "       1: getstatic  // Field GamePacketTypes.SERVERBOUND_B\n"
        "  private static void lambda$static$2(x);\n"
        "    Code:\n"
        "       0: getstatic  // Field GamePacketTypes.CLIENTBOUND_A\n"
        "       1: getstatic  // Field GamePacketTypes.CLIENTBOUND_B\n"
    )
    # The real floors are 100/50; a synthetic fixture cannot meet them, so the
    # shape assertions are exercised here and the floors are exercised against
    # the real jar by every ordinary run.
    refuses("parse_protocol: below floor", lambda: parse_protocol(good), "extracted only")
    refuses(
        "parse_protocol: one lambda registering both directions",
        lambda: parse_protocol(good.replace("CLIENTBOUND_B", "SERVERBOUND_B")),
        "registers both directions",
    )
    refuses(
        "parse_protocol: a packet registered twice",
        lambda: parse_protocol(good.replace("CLIENTBOUND_B", "CLIENTBOUND_A")),
        "registered twice",
    )
    refuses("parse_protocol: no lambdas at all", lambda: parse_protocol("nothing here"), "could not find both registration lambdas")

    # --- ids_rs -------------------------------------------------------------
    refuses("ids_rs: no `pub mod play`", lambda: ids_rs("pub mod login { }"), "could not find `pub mod play`")
    refuses(
        "ids_rs: play present, cb/sb missing",
        lambda: ids_rs("pub mod play {\n    pub mod other {\n    }\n}"),
        "could not find `pub mod",
    )
    body = "pub mod play {\n    pub mod sb {\n%s\n    }\n    pub mod cb {\n%s\n    }\n}"
    thin = body % (
        "\n".join(f"        pub const S{i}: i32 = {i};" for i in range(5)),
        "\n".join(f"        pub const C{i}: i32 = {i};" for i in range(5)),
    )
    refuses("ids_rs: below floor", lambda: ids_rs(thin), "extracted only")

    # --- trace_sets ---------------------------------------------------------
    refuses("trace_sets: no FAMILIES", lambda: trace_sets("fn decode() {}"), "could not find `pub const FAMILIES`")
    hk = ("pub const FAMILIES: &[Family] = &[\n"
          + "".join(
              "    Family {\n        id: Some(ids::play::cb::H%d),\n"
              "        kind: \"h%d\",\n        class: Class::Ambient,\n        arm: false,\n    }\n" % (i, i)
              for i in range(12)
          )
          + "];\n")
    refuses(
        "ambient_ids: a table with too few rows",
        lambda: trace_sets("pub const FAMILIES: &[Family] = &[\n    Family {\n        id: None,\n    }\n];\n"),
        "matched only",
    )
    refuses(
        "ambient_ids: a table whose rows are all graded",
        lambda: trace_sets(hk.replace("Class::Ambient", "Class::Graded")),
        "declares only 0 ambient",
    )
    refuses("trace_sets: no decode", lambda: trace_sets(hk), "could not find `fn decode`")
    refuses(
        "trace_sets: decode with too few arms",
        lambda: trace_sets(hk + 'fn decode(\n) {\n x if x == cb::A => { "a".into() }\n}\n'),
        "yielded only",
    )

    # --- client_sends -------------------------------------------------------
    refuses("client_sends: empty source", lambda: client_sends("fn main() {}"), "mentions only")

    # --- proto_drift --------------------------------------------------------
    jar_cb, jar_sb = ["BUNDLE", "ADD_ENTITY", "ANIMATE"], ["ACCEPT_TELEPORTATION", "ATTACK"]
    ours_cb = {"BUNDLE_DELIMITER": 0, "ADD_ENTITY": 1, "ANIMATE": 2}
    ours_sb = {"ACCEPT_TELEPORTATION": 0, "ATTACK": 1}
    d = accepts("proto_drift: agreeing ids", lambda: proto_drift(jar_cb, jar_sb, ours_cb, ours_sb))
    if d:
        failures.append(f"proto_drift: reported drift on agreeing ids — {d}")
    moved = dict(ours_cb, ANIMATE=1)
    if not proto_drift(jar_cb, jar_sb, moved, ours_sb):
        failures.append("proto_drift: a moved id did NOT register as drift")
    if not proto_drift(jar_cb, jar_sb, dict(ours_cb, EXTRA=99), ours_sb):
        failures.append("proto_drift: an id past the end of the jar list did NOT register")

    # The alias is load-bearing: without it, the real run refuses. Prove the
    # entry is doing work rather than sitting there.
    saved = dict(ALIASES)
    ALIASES.clear()
    try:
        if not proto_drift(jar_cb, jar_sb, ours_cb, ours_sb):
            failures.append("ALIASES: BUNDLE_DELIMITER is not actually needed — drop it")
    finally:
        ALIASES.clear()
        ALIASES.update(saved)

    for f in failures:
        print(f"corpus-surface selftest: FAIL — {f}", file=sys.stderr)
    if failures:
        return 1
    print("corpus-surface selftest: OK — 11 refusals fired for their own reason, 4 cross-checks agreed")
    return 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jar", default=JAR)
    ap.add_argument(
        "--recordings",
        default="~/rustmc*/oracle-work/gameplay/vanilla.json",
        help="glob of recorder outputs to census; '' to skip",
    )
    ap.add_argument("--quiet", action="store_true", help="summary only")
    ap.add_argument("--selftest", action="store_true", help="exercise every refusal and exit")
    args = ap.parse_args()

    if args.selftest:
        sys.exit(selftest())

    jar_cb, jar_sb = jar_protocol(args.jar)
    our_cb, our_sb = ids_rs()
    housekeeping, decoded = trace_sets()
    sends = client_sends()
    checks = load_checks()

    # --- the cross-check that makes the rest of this trustworthy ------------
    #
    # Registration order is only the id order if our own constants agree, and
    # if they do not then every row below is attributed to the wrong packet.
    # Disagreement is fatal.
    drift = proto_drift(jar_cb, jar_sb, our_cb, our_sb)
    if drift:
        print("corpus-surface: REFUSED — rustmc-proto disagrees with the pinned jar:", file=sys.stderr)
        for d in drift:
            print(f"  {d}", file=sys.stderr)
        sys.exit(2)

    used_kinds = defaultdict(list)
    used_steps = defaultdict(list)
    for fn, c in checks.items():
        for k in c.get("observe", []):
            used_kinds[k].append(fn)
        for s in c.get("setup", []) + c.get("step", []):
            used_steps[s["do"]].append(fn)

    if args.recordings:
        files, emitted, where, per = recordings_census(args.recordings)
    else:
        files, emitted, where, per = [], Counter(), {}, {}

    # A kind is observed if any check names it. Map that back onto packets.
    kind_of = decoded  # cb name -> kind
    opaque_kind = lambda i: f"packet:0x{i:02X}"  # noqa: E731

    # Rows are keyed by the **jar's** name — it is the primary source — but
    # every set of ours is keyed by ours, so an alias has to be undone before
    # any lookup. Getting this wrong would file an aliased packet as opaque
    # when it is decoded, which is the direction that invents a coverage hole.
    ours_name = {v: k for k, v in ALIASES.items()}

    rows = []
    for i, name in enumerate(jar_cb):
        mine = ours_name.get(name, name)
        if mine in housekeeping:
            tier, kind = "housekeeping", None
        elif mine in kind_of:
            tier, kind = "legible", kind_of[mine]
        else:
            tier, kind = "opaque", opaque_kind(i)
        rows.append(
            {
                "id": i,
                "name": name,
                "tier": tier,
                "kind": kind,
                "named": mine in our_cb,
                "checks": len(used_kinds.get(kind, [])) if kind else 0,
                "emitted": emitted.get(kind, 0) if kind else 0,
            }
        )

    # -----------------------------------------------------------------------
    print(f"pinned jar   {os.path.relpath(args.jar, ROOT)}")
    print(f"corpus       {len(checks)} checks under ci/checks/gameplay")
    if files:
        print(f"recordings   {len(files)} vanilla runs censused")
    else:
        print("recordings   none matched — the `emitted` column is omitted, not zero")
    print()

    n_hk = sum(1 for r in rows if r["tier"] == "housekeeping")
    n_leg = sum(1 for r in rows if r["tier"] == "legible")
    n_opq = sum(1 for r in rows if r["tier"] == "opaque")
    n_obs = sum(1 for r in rows if r["checks"])
    n_emit = sum(1 for r in rows if r["emitted"])
    n_emit_obs = sum(1 for r in rows if r["emitted"] and r["checks"])

    print("== clientbound play surface ==")
    print(f"  {len(jar_cb):3d}  in the protocol             (GameProtocols, pinned jar)")
    print(f"  {sum(1 for r in rows if r['named']):3d}  named in rustmc-proto")
    print(f"  {n_hk:3d}  ambient by declaration      (trace::FAMILIES)")
    print(f"  {n_leg:3d}  legible                     (a trace::decode arm)")
    print(f"  {n_opq:3d}  opaque                      (recorded as packet:0xNN, body digested)")
    print(f"  {n_obs:3d}  observed by >= 1 check")
    if files:
        print(f"  {n_emit:3d}  EMITTED by vanilla during a corpus run")
        print(f"  {n_emit_obs:3d}  emitted AND observed  <- the corpus's real clientbound reach")
    print()

    if files and not args.quiet:
        print("== emitted but unobserved — vanilla speaks, no check listens ==")
        print("   (these need no new scenario; the recording already exists)")
        for r in sorted(rows, key=lambda r: -r["emitted"]):
            if r["emitted"] and not r["checks"]:
                n = len(where.get(r["kind"], ()))
                print(f"  0x{r['id']:02X} {r['name']:34s} {r['tier']:12s} {r['emitted']:6d} in {n:2d} scenarios")
        print()

        gap_kinds = [r["kind"] for r in sorted(rows, key=lambda r: -r["emitted"]) if r["emitted"] and not r["checks"]]
        print("== is that free coverage stable? — the same recordings, per scenario ==")
        print("   (a candidate projection is screened here before a check exists to ask)")
        for k, name, n, verdict in stability(files, per, gap_kinds):
            print(f"  {k:18s} {name:52s} {verdict}")
        print()

        print("== observed but never emitted — a projection with nothing behind it ==")
        for k in sorted(used_kinds):
            if emitted.get(k, 0) == 0:
                print(f"  {k:34s} projected by {len(used_kinds[k])} check(s), 0 occurrences")
        print()

    if not args.quiet:
        print("== legible but no check projects it ==")
        for r in rows:
            if r["tier"] == "legible" and not r["checks"]:
                e = f"{r['emitted']} emitted" if files else "unmeasured"
                print(f"  0x{r['id']:02X} {r['name']:34s} -> {r['kind']:24s} {e}")
        print()

        print("== dropped before any decoder can see it ==")
        for r in rows:
            if r["tier"] == "housekeeping":
                print(f"  0x{r['id']:02X} {r['name']}")
        print()

    # -----------------------------------------------------------------------
    reachable = {n for n in jar_sb if n in sends}
    print("== serverbound play surface ==")
    print(f"  {len(jar_sb):3d}  in the protocol             (GameProtocols, pinned jar)")
    print(f"  {sum(1 for n in jar_sb if n in our_sb):3d}  named in rustmc-proto")
    print(f"  {len(reachable):3d}  client.rs can send          <- the harness's whole vocabulary")
    print()
    if not args.quiet:
        print("== serverbound the harness cannot send at all ==")
        print("   (each of these is a harness item, not a scenario item)")
        for i, n in enumerate(jar_sb):
            if n not in sends:
                print(f"  0x{i:02X} {n}")
        print()

    held = {s["item"] for c in checks.values() for s in c.get("setup", []) + c.get("step", []) if s["do"] == "hold"}
    beh = behaviour_tables(held, Hierarchy(args.jar))
    print("== block-behaviour surface, from the jar-derived tables ==")
    if beh is None:
        print("  data/vanilla absent — generate it with ci/gen-gameplay-tables.sh")
    else:
        scored, skipped = beh
        print(f"  the corpus holds {len(held)} distinct items, and can involve no block it does not hold")
        print(f"  {'table':24s} {'blocks':>14s} {'classes':>14s}")
        for name, nb, hb, nc, hc, resolved in scored:
            tag = "" if resolved else "   (runtime classes — not collapsed onto implementations)"
            print(f"  {name:24s} {hb:5d} of {nb:5d}   {hc:5d} of {nc:5d}{tag}")
        for name, t in skipped:
            print(f"  {name:24s} skipped — `class` is {t}, not a class name")
    print()

    print("== the headline ==")
    if files:
        print(
            f"  clientbound: {n_emit_obs} of {n_emit} packet types vanilla actually emits "
            f"are looked at by a check"
        )
    print(
        f"  clientbound: {n_obs} of {len(jar_cb)} in the protocol are looked at by a check"
    )
    print(f"  serverbound: {len(reachable)} of {len(jar_sb)} in the protocol can be sent at all")


if __name__ == "__main__":
    main()
