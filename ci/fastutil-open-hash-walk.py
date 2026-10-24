#!/usr/bin/env python3
"""fastutil-open-hash-walk.py — the ten inputs that decide the walk order of
`Int2ObjectOpenHashMap`, read out of the shipped fastutil's own bytecode.

# Why this file exists rather than a comment saying "we ported fastutil"

§101 measured vanilla's per-tick broadcast order as a golden table and refused
to reimplement the hash, on the ground that "a hash written from memory that is
right for {1,2} and wrong for {1,2,7} is worse than no hash". §103 ports it, so
that ground has to be answered rather than inherited.

The answer is not "we agree on the 41 pinned sets". A walk order is a function
of ten separate things, and a reimplementation can reproduce nine of them and
agree on every set anyone happened to write down. This file names all ten,
reads each one from the bytecode of the exact jar the server runs against, and
pins it. `crates/rustmc-server/src/openhash.rs` reproduces this pin and cites
it fact by fact, so "which inputs did you reproduce" has a file for an answer
instead of a claim.

# The ten, and what each one decides

| # | input | decides |
|---|---|---|
| 1 | `HashCommon.mix(int)` | which slot a key wants |
| 2 | `HashCommon.nextPowerOfTwo(long)` | the table sizes that exist |
| 3 | `HashCommon.arraySize(int, float)` | the table size for an expected count |
| 4 | `HashCommon.maxFill(int, float)` | when the table grows |
| 5 | the no-argument constructor's `(expected, f)` | the size it starts at |
| 6 | `find`'s probe step | where a colliding key lands |
| 7 | `insert`'s growth rule | the size after n puts |
| 8 | `removeEntry`'s shrink rule | the size after puts AND removes |
| 9 | `shiftKeys` | that a remove leaves no tombstone |
| 10 | `MapIterator`'s start and direction | the order the survivors come out in |

Nine of those are about the table's *state*, and exactly one — the last — is
about the walk. That is the trap this file exists to close: an implementation
can get the iterator exactly right and still walk a different order, because it
built a different table to walk.

Item 8 is the one a golden table of insert-only sets cannot see at all.
`removeEntry` shrinks the table when the map has both grown and then emptied
out, so **the walk is a pure function of the key set only at a fixed capacity**,
and the capacity is a function of the whole history. §101 said "a pure function
of the key set"; that is true for every set it measured and false in general,
and the correction is §103.2.

# What --check refuses

Every reader dies loudly. A javap that produces nothing, a method whose body no
longer contains the anchor, a constant that moved: each is a refusal with an
empty stdout, not a quiet pass, because every headline here is a SAMENESS and a
dead reader reports sameness for free.
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
FASTUTIL_REL = ("oracle-work/vanilla-26.2/libraries/it/unimi/dsi/fastutil/"
                "8.5.18/fastutil-8.5.18.jar")
JAR_REL = "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
PIN = os.path.join(HERE, "checks", "fastutil-open-hash-walk.json")
GOLDEN = os.path.join(HERE, "checks", "fastutil-walk-history-golden.txt")
PROBE = os.path.join(HERE, "probes", "WalkHistory.java")
OURS = os.path.join(ROOT, "crates", "rustmc-server", "src", "openhash.rs")

# The corpus seed. Fixed so the histories are the same ones every time this is
# re-measured, which is what makes "the golden did not move" a statement about
# fastutil rather than about the draw.
CORPUS_SEED = 20260819


class Lcg:
    """A named LCG rather than `random`, so the corpus is reproducible on any
    Python. `random.sample`'s internals have changed between releases before,
    and a golden that regenerates differently on a different interpreter is a
    golden nobody can re-measure."""

    def __init__(self, seed):
        self.s = seed & 0xFFFFFFFFFFFFFFFF

    def next(self):
        self.s = (self.s * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
        return (self.s >> 33) & 0x7FFFFFFF

    def below(self, n):
        return self.next() % n

    def between(self, lo, hi):
        return lo + self.below(hi - lo + 1)


def corpus():
    """The histories to measure, in five families.

    Each family exists to reach an input the others cannot:

    * `prefix` walks every table size boundary the growth rule has;
    * `pairs` is every two-entity set this server can build from small ids,
      which is the case that is live today;
    * `sets` is insert-only at random, the shape §101.5's golden already had;
    * `mixed` interleaves removes, which reaches `shiftKeys`;
    * `drain` grows the table and then empties it, which is the only family
      that reaches the shrink rule — and therefore the only one that can tell
      a port from a reimplementation that assumed the walk is a function of
      the key set alone.
    """
    r = Lcg(CORPUS_SEED)
    out = []
    for k in range(1, 65):
        out.append(["+%d" % i for i in range(1, k + 1)])
    for a in range(1, 17):
        for b in range(a + 1, 17):
            out.append(["+%d" % a, "+%d" % b])
    for _ in range(300):
        size = r.between(1, 40)
        ids = []
        while len(ids) < size:
            k = r.between(1, 200)
            if k not in ids:
                ids.append(k)
        out.append(["+%d" % i for i in ids])
    for _ in range(400):
        ops = []
        live = []
        for _ in range(r.between(10, 80)):
            if live and r.below(100) < 30:
                v = live.pop(r.below(len(live)))
                ops.append("-%d" % v)
            else:
                k = r.between(1, 64)
                if k not in live:
                    live.append(k)
                ops.append("+%d" % k)
        out.append(ops)
    for k in (25, 33, 40, 49, 65, 100, 129, 200):
        for _ in range(8):
            ops = ["+%d" % i for i in range(1, k + 1)]
            live = list(range(1, k + 1))
            for _ in range(int(k * 0.85)):
                v = live.pop(r.below(len(live)))
                ops.append("-%d" % v)
            out.append(ops)
    return out

MAP = "it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap"
ITER = "it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap$MapIterator"
COMMON = "it.unimi.dsi.fastutil.HashCommon"


def die(msg):
    print("fastutil-open-hash-walk: REFUSED — %s" % msg, file=sys.stderr)
    sys.exit(3)


def javap(jar, cls):
    try:
        out = subprocess.run(
            ["javap", "-p", "-c", "-constants", "-classpath", jar, cls],
            capture_output=True, text=True, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        die("javap could not run (%s)" % e)
    if out.returncode != 0 or not out.stdout.strip():
        die("javap produced nothing for %s" % cls)
    return out.stdout


def body(dump, signature, what):
    """One method's bytecode, by its javap signature line."""
    lines = dump.splitlines()
    for i, ln in enumerate(lines):
        if ln.strip() == signature:
            out = []
            for j in range(i + 1, len(lines)):
                if lines[j].strip() == "" and out:
                    break
                out.append(lines[j])
            if not out:
                die("%s has an empty body — the reader is dead" % what)
            return "\n".join(out)
    die("%s not found in the jar (looked for %r) — the reader is dead"
        % (what, signature))
    return ""  # unreachable


def ints(text):
    """Every integer constant the body pushes.

    Two forms, and reading only one of them is how this reader first went
    blind: a wide constant arrives through the pool as `ldc // int N`, but a
    small one — a shift distance, a divisor, a table floor — is a `bipush` or
    `sipush` with its value as the operand and no comment at all. `mix`'s
    multiplier is the first kind and its `>>> 16` is the second.
    """
    out = [int(m) for m in re.findall(r"//\s+(?:int|long)\s+(-?\d+)l?\b", text)]
    out += [int(m) for m in re.findall(r"^\s*\d+:\s+[bs]ipush\s+(-?\d+)", text, re.M)]
    return out


def opcodes(text):
    return [m.group(1) for m in re.finditer(r"^\s*\d+:\s+(\S+)", text, re.M)]


def read_mix(dump):
    """1. mix(int): h = k * INT_PHI; return h ^ (h >>> 16)."""
    b = body(dump, "public static int mix(int);", "HashCommon.mix(int)")
    consts = ints(b)
    ops = opcodes(b)
    if "imul" not in ops or "iushr" not in ops or "ixor" not in ops:
        die("HashCommon.mix(int) is no longer multiply/shift/xor (%s)"
            % " ".join(ops))
    phi = [c for c in consts if abs(c) > 1000]
    if len(phi) != 1:
        die("HashCommon.mix(int) has %d wide constants, expected exactly the "
            "multiplier: %r" % (len(phi), consts))
    shifts = [c for c in consts if 0 < c < 64]
    if len(shifts) != 1:
        die("HashCommon.mix(int) has %d shift constants: %r" % (len(shifts), consts))
    return {"multiplier": phi[0], "shift": shifts[0], "op": "h ^ (h >>> shift)"}


def read_next_power_of_two(dump):
    """2. nextPowerOfTwo(long): 1L << (64 - numberOfLeadingZeros(x - 1))."""
    b = body(dump, "public static long nextPowerOfTwo(long);",
             "HashCommon.nextPowerOfTwo(long)")
    if "Long.numberOfLeadingZeros" not in b:
        die("HashCommon.nextPowerOfTwo(long) no longer counts leading zeros")
    ops = opcodes(b)
    if "lshl" not in ops:
        die("HashCommon.nextPowerOfTwo(long) no longer shifts (%s)" % " ".join(ops))
    width = [c for c in ints(b) if c in (32, 64)]
    if width != [64]:
        die("HashCommon.nextPowerOfTwo(long)'s width constant is %r, not 64" % width)
    return {"expr": "1L << (64 - Long.numberOfLeadingZeros(x - 1))"}


def read_array_size(dump):
    """3. arraySize(expected, f) = max(2, nextPowerOfTwo(ceil(expected / f)))."""
    b = body(dump, "public static int arraySize(int, float);",
             "HashCommon.arraySize(int, float)")
    for want in ("Math.ceil", "nextPowerOfTwo", "Math.max"):
        if want not in b:
            die("HashCommon.arraySize no longer calls %s" % want)
    ops = opcodes(b)
    if "ddiv" not in ops:
        die("HashCommon.arraySize no longer divides expected by the load factor")
    floors = [c for c in ints(b) if c == 2]
    if not floors:
        die("HashCommon.arraySize's floor of 2 is gone")
    cap = [c for c in ints(b) if c == 1073741824]
    if not cap:
        die("HashCommon.arraySize's 2^30 ceiling is gone")
    return {"expr": "max(2, nextPowerOfTwo(ceil(expected / f)))",
            "floor": 2, "ceiling": 1073741824}


def read_max_fill(dump):
    """4. maxFill(n, f) = min(ceil(n * f), n - 1)."""
    b = body(dump, "public static int maxFill(int, float);",
             "HashCommon.maxFill(int, float)")
    for want in ("Math.ceil", "Math.min"):
        if want not in b:
            die("HashCommon.maxFill no longer calls %s" % want)
    ops = opcodes(b)
    if "dmul" not in ops or "isub" not in ops:
        die("HashCommon.maxFill is no longer min(ceil(n*f), n-1) (%s)" % " ".join(ops))
    return {"expr": "min(ceil(n * f), n - 1)"}


def read_default_ctor(dump):
    """5. The no-argument constructor's expected size and load factor."""
    b = body(dump, "public %s();" % MAP, "%s.<init>()" % MAP)
    if '<init>":(IF)V' not in b:
        die("the no-arg constructor no longer delegates to (int, float)")
    expected = re.search(r"bipush\s+(\d+)", b)
    load = re.search(r"//\s+float\s+([0-9.]+)f", b)
    if expected is None or load is None:
        die("could not read the no-arg constructor's (expected, f) — the "
            "reader is dead")
    return {"expected": int(expected.group(1)), "load_factor": float(load.group(1))}


def read_probe(dump):
    """6. find(): slot = mix(k) & mask, then linear +1 & mask."""
    b = body(dump, "private int find(int);", "%s.find(int)" % MAP)
    if "HashCommon.mix" not in b:
        die("find() no longer mixes the key")
    masks = len(re.findall(r"Field mask:I", b))
    if masks < 2:
        die("find() reads the mask %d times; a linear probe reads it for the "
            "first slot and again for every step" % masks)
    # The step itself: `iconst_1 / iadd` between two mask reads.
    if not re.search(r"iconst_1\s*\n\s*\d+:\s+iadd", b):
        die("find()'s probe step is no longer +1 — this is not linear probing")
    if "Field containsNullKey:Z" not in b:
        die("find() no longer special-cases the zero key")
    return {"slot": "mix(k) & mask", "step": "+1 & mask", "kind": "linear"}


def read_growth(dump):
    """7. insert(): if (size++ >= maxFill) rehash(arraySize(size + 1, f))."""
    b = body(dump, "private void insert(int, int, V);", "%s.insert" % MAP)
    for want in ("Field maxFill:I", "HashCommon.arraySize", "Method rehash:(I)V"):
        if want not in b:
            die("insert() no longer references %s" % want)
    if "if_icmplt" not in opcodes(b):
        die("insert()'s growth test is no longer a less-than against maxFill")
    if "Field containsNullKey:Z" not in b:
        die("insert() no longer flags the zero key's slot")
    return {"test": "size_before >= maxFill", "target": "arraySize(size_after + 1, f)"}


def read_shrink(dump):
    """8. removeEntry(): if (n > minN && size < maxFill/4 && n > 16) rehash(n/2)."""
    b = body(dump, "private V removeEntry(int);", "%s.removeEntry" % MAP)
    for want in ("Field minN:I", "Field maxFill:I", "Method shiftKeys:(I)V",
                 "Method rehash:(I)V"):
        if want not in b:
            die("removeEntry() no longer references %s" % want)
    consts = ints(b)
    if 16 not in consts:
        die("removeEntry()'s floor of 16 is gone (constants %r)" % consts)
    if "idiv" not in opcodes(b):
        die("removeEntry() no longer divides — the quarter-fill test is gone")
    return {"test": "n > minN and size < maxFill / 4 and n > 16",
            "target": "n / 2", "floor": 16}


def read_shift_keys(dump):
    """9. shiftKeys(): Knuth algorithm R, so a remove leaves no tombstone."""
    b = body(dump, "protected final void shiftKeys(int);", "%s.shiftKeys" % MAP)
    if "HashCommon.mix" not in b:
        die("shiftKeys() no longer re-mixes the keys it walks over")
    ops = opcodes(b)
    for want in ("if_icmpgt", "if_icmpge", "if_icmple"):
        if want not in ops:
            die("shiftKeys()'s wrap-aware comparison is gone (%s missing)" % want)
    return {"kind": "knuth algorithm R shift-back", "tombstones": False}


def read_iterator(dump_iter):
    """10. MapIterator: start at n, walk DOWN, zero key first."""
    ctor = body(dump_iter, "private %s(%s);" % (ITER, MAP), "MapIterator.<init>")
    m = re.search(r"getfield\s+#\d+\s+// Field \S*?\.n:I\s*\n\s*\d+:\s+putfield\s+#\d+\s+// Field pos:I",
                  ctor)
    if m is None:
        die("MapIterator no longer starts `pos` at `n` — the reader is dead")
    # Qualified in this dump — the iterator is a nested class reading the
    # map's field — so match on the field rather than on javap's rendering of
    # the owner, which differs between the two classes for the same field.
    if "containsNullKey:Z" not in ctor or "mustReturnNullKey:Z" not in ctor:
        die("MapIterator no longer arms the zero-key-first flag")
    nxt = body(dump_iter, "public int nextEntry();", "MapIterator.nextEntry")
    if not re.search(r"getfield\s+#\d+\s+// Field pos:I\s*\n\s*\d+:\s+iconst_1\s*\n\s*\d+:\s+isub",
                     nxt):
        die("MapIterator.nextEntry no longer decrements `pos` — the walk "
            "direction is the one thing this file cannot infer from anything else")
    if "Field mustReturnNullKey:Z" not in nxt:
        die("MapIterator.nextEntry no longer returns the zero key first")
    return {"start": "pos = n", "direction": "descending",
            "zero_key": "returned first, from slot n"}


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as fh:
        for b in iter(lambda: fh.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def measure_walks(fu, histories):
    """Run the history probe against the pinned jar's own fastutil."""
    if not os.path.exists(PROBE):
        die("the probe is missing: %s" % PROBE)
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        r = subprocess.run(["javac", "-cp", fu, "-d", td, PROBE],
                           capture_output=True, text=True, timeout=600)
        if r.returncode != 0 or not os.path.exists(os.path.join(td, "WalkHistory.class")):
            die("the probe did not compile: %s" % (r.stderr or "").strip()[:400])
        stdin = "\n".join(" ".join(h) for h in histories) + "\n"
        r = subprocess.run(["java", "-cp", td + os.pathsep + fu, "WalkHistory"],
                           input=stdin, capture_output=True, text=True, timeout=600)
        if r.returncode != 0 or not r.stdout.strip():
            die("the probe produced nothing: %s" % (r.stderr or "").strip()[:400])
    rows = []
    for line in r.stdout.strip().splitlines():
        ops, walk, obj = line.split("|")
        # THE CLAIM BEHIND "ONE WALK, TWO KEYHASH SITES", MEASURED.
        # `PlayerMap.players` is an `Object2BooleanOpenHashMap<ServerPlayer>`
        # and `Entity.hashCode()` returns `getId()`, from which §101 concluded
        # the player set walks the same numbers in the same order as
        # `ChunkMap.entityMap`. That is a claim about two different generated
        # classes agreeing — same `HashCommon.mix`, same probe, same
        # descending iterator, but a `null` slot where the int map has a zero
        # key. It holds on every history measured here, and if it ever stops
        # holding this refuses rather than pinning a walk that is right for
        # one of the two sites.
        if walk != obj:
            die("the object-keyed map walked {%s} as [%s] where the int-keyed "
                "map walked [%s] — `Entity.hashCode() == getId()` no longer "
                "makes the two KEYHASH sites one order" % (ops, obj, walk))
        rows.append((ops, walk, obj))
    if len(rows) != len(histories):
        die("the probe answered %d of %d histories" % (len(rows), len(histories)))
    for (got, _, _), want in zip(rows, histories):
        if got.split() != want:
            die("the probe echoed a history it was not given — it is not "
                "answering the corpus this file generated")
    return rows


def read_golden():
    """The measured walks, and the anti-vacuity tests on them.

    Every number below is a floor, not a target. They exist because a golden
    file is exactly the kind of thing that can rot into a green: an empty one,
    a truncated one, or one where every walk happens to be the identity would
    all let a wrong port pass a replay test without a single assertion firing.
    """
    if not os.path.exists(GOLDEN):
        die("no walk golden at %s; run --measure once and commit it" % GOLDEN)
    header = {}
    rows = []
    for line in open(GOLDEN, encoding="utf-8"):
        line = line.rstrip("\n")
        if line.startswith("#"):
            if ":" in line:
                k, v = line[1:].split(":", 1)
                header[k.strip()] = v.strip()
            continue
        if not line:
            continue
        ops, walk, obj = line.split("|")
        if walk != obj:
            die("a golden row has the object-keyed map walking a different "
                "order from the int-keyed one: {%s}" % ops)
        rows.append((ops.split(), [int(x) for x in walk.split()] if walk else []))
    if len(rows) < 900:
        die("the walk golden holds %d histories; a corpus that has shrunk is "
            "a corpus that stopped covering something" % len(rows))
    permuted = sum(1 for ops, walk in rows if walk != sorted(walk))
    if permuted < len(rows) // 4:
        die("only %d of %d golden walks are a real permutation of their keys; "
            "a probe returning its input unchanged would look like this"
            % (permuted, len(rows)))
    with_removes = sum(1 for ops, _ in rows if any(o[0] == "-" for o in ops))
    if with_removes < 200:
        die("only %d golden histories contain a remove, so the shrink and "
            "shift-back rules are barely covered" % with_removes)
    drained = sum(1 for ops, walk in rows
                  if sum(1 for o in ops if o[0] == "+") > 24 and len(walk) < 8)
    if drained < 30:
        die("only %d golden histories grow past the first table boundary and "
            "then drain, so the shrink rule is not covered at all" % drained)
    return header, rows


def read_all(fu):
    common = javap(fu, COMMON)
    mp = javap(fu, MAP)
    it = javap(fu, ITER)
    return {
        "1_mix": read_mix(common),
        "2_next_power_of_two": read_next_power_of_two(common),
        "3_array_size": read_array_size(common),
        "4_max_fill": read_max_fill(common),
        "5_default_ctor": read_default_ctor(mp),
        "6_probe": read_probe(mp),
        "7_growth": read_growth(mp),
        "8_shrink": read_shrink(mp),
        "9_shift_keys": read_shift_keys(mp),
        "10_iterator": read_iterator(it),
    }


def read_the_users(jar):
    """The two sites in the SERVER that walk one of these maps.

    A parameter table nothing consumes is a parameter table, not a finding.
    These are the two constructions §101.5 rests on, re-read here so this file
    goes red if the server stops using the container it describes.
    """
    cm = javap(jar, "net.minecraft.server.level.ChunkMap")
    lines = cm.splitlines()
    entity_map = None
    for i, ln in enumerate(lines):
        if re.search(r"putfield\s+#\d+\s+// Field entityMap:", ln):
            for j in range(i - 1, max(-1, i - 20), -1):
                m = re.search(r"new\s+#\d+\s+// class ([a-zA-Z0-9/$]+)", lines[j])
                if m:
                    entity_map = m.group(1).rsplit("/", 1)[-1]
                    break
            for j in range(i - 1, max(-1, i - 20), -1):
                if "Int2ObjectOpenHashMap.\"<init>\":()V" in lines[j]:
                    break
            else:
                die("ChunkMap.entityMap is no longer built with the no-argument "
                    "constructor, so the pinned starting size is the wrong one")
            break
    if entity_map is None:
        die("could not find ChunkMap.entityMap's construction — the reader is dead")

    # AND WHAT EACH WALK IN `ChunkMap.tick` ACTUALLY FEEDS.
    #
    # §101.5 filed `PlayerMap.players` as "the order players are walked in for
    # every broadcast". The bytecode does not say that. `ChunkMap.tick` walks
    # THREE collections and they go to three different places, so the call
    # sequence is derived here rather than described, and pinned:
    #
    #   playerMap.getAllPlayers()  ->  updateChunkTracking(ServerPlayer)
    #   ServerLevel.players()      ->  TrackedEntity.updatePlayers(List)
    #   entityMap.values()         ->  updatePlayers / sendChanges
    #
    # `updatePlayers` takes a `java.util.List`, and the list it is handed is
    # `ServerLevel.players()`, which is a plain `List` field — INSERTION order,
    # not the player map's hash. So the hash order over player ids reaches
    # chunk tracking and does not reach the entity broadcast at all.
    try:
        i = cm.index("  protected void tick();")
        tick = cm[i:cm.index("\n\n", i)]
    except ValueError:
        die("ChunkMap.tick not found — the call-sequence reader is dead")
    interesting = ("PlayerMap.getAllPlayers", "updateChunkTracking",
                   "ServerLevel.players", "Int2ObjectMap.values",
                   "TrackedEntity.updatePlayers", "ServerEntity.sendChanges")
    seq = []
    for m in re.finditer(r"//\s+(?:Interface)?Method\s+(\S+?):", tick):
        name = m.group(1).replace("/", ".").replace("$", ".")
        short = ".".join(name.split(".")[-2:])
        if short in interesting and (not seq or seq[-1] != short):
            seq.append(short)
    if not seq:
        die("ChunkMap.tick names none of the calls this finding rests on — "
            "the call-sequence reader is dead")
    for want in ("PlayerMap.getAllPlayers", "updateChunkTracking",
                 "ServerLevel.players", "TrackedEntity.updatePlayers"):
        if want not in seq:
            die("ChunkMap.tick no longer calls %s, so the derivation of which "
                "walk feeds which consumer has moved" % want)

    pm = javap(jar, "net.minecraft.server.level.PlayerMap")
    player_map = None
    for i, ln in enumerate(pm.splitlines()):
        if re.search(r"putfield\s+#\d+\s+// Field players:", ln):
            for j in range(i - 1, max(-1, i - 12), -1):
                m = re.search(r"new\s+#\d+\s+// class ([a-zA-Z0-9/$]+)",
                              pm.splitlines()[j])
                if m:
                    player_map = m.group(1).rsplit("/", 1)[-1]
                    break
            break
    if player_map is None:
        die("could not find PlayerMap.players' construction — the reader is dead")
    return {"ChunkMap.entityMap": entity_map, "PlayerMap.players": player_map,
            "ChunkMap.tick call sequence": " -> ".join(seq)}


def read_ours():
    """Ours, and only the fact that it cites this pin.

    This file does not grade our walk; running it is what grades it, and
    `ci/entity-broadcast-order.py` does that by executing it. What is checked
    here is narrower and still worth checking: that our port names every one of
    the ten inputs, so a parameter that moves in the jar has a place in our
    source to be wrong.
    """
    if not os.path.exists(OURS):
        die("our port is missing: %s" % OURS)
    src = open(OURS, encoding="utf-8").read()
    missing = [k for k in ("1_mix", "2_next_power_of_two", "3_array_size",
                           "4_max_fill", "5_default_ctor", "6_probe",
                           "7_growth", "8_shrink", "9_shift_keys",
                           "10_iterator") if k not in src]
    if missing:
        die("our port does not cite these pinned inputs, so nothing in the "
            "source would have to change if they moved: %s" % ", ".join(missing))
    return {"source": os.path.relpath(OURS, ROOT), "cites_all_ten": True}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--measure", action="store_true")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    fu = os.environ.get("RUSTMC_FASTUTIL") or os.path.join(ROOT, FASTUTIL_REL)
    if not os.path.exists(fu):
        die("no fastutil under %s" % fu)
    jar = os.environ.get("RUSTMC_JAR") or os.path.join(ROOT, JAR_REL)
    if not os.path.exists(jar):
        die("no pinned server jar under %s" % jar)

    facts = read_all(fu)
    users = read_the_users(jar)
    ours = read_ours()

    if args.measure:
        pin = {"fastutil_path": FASTUTIL_REL, "fastutil_sha256": sha256(fu),
               "inputs": facts, "users": users}
        with open(PIN, "w", encoding="utf-8") as fh:
            json.dump(pin, fh, indent=2, sort_keys=True)
            fh.write("\n")
        histories = corpus()
        rows = measure_walks(fu, histories)
        with open(GOLDEN, "w", encoding="utf-8") as fh:
            fh.write("# fastutil_path: %s\n" % FASTUTIL_REL)
            fh.write("# fastutil_sha256: %s\n" % sha256(fu))
            fh.write("# corpus_seed: %d\n" % CORPUS_SEED)
            fh.write("# histories: %d\n" % len(rows))
            fh.write("# generated_by: ci/fastutil-open-hash-walk.py --measure\n")
            fh.write("# probe: ci/probes/WalkHistory.java\n")
            fh.write("#\n")
            fh.write("# One history per line as `ops|walk|object-keyed walk`. `+k` puts key\n")
            fh.write("# k, `-k` removes it; `walk` is what Int2ObjectOpenHashMap.values()\n")
            fh.write("# yields afterwards, which is the order `ChunkMap.tick()` broadcasts\n")
            fh.write("# in. The third column is the same ids out of an\n")
            fh.write("# Object2BooleanOpenHashMap keyed by an object whose hashCode is the\n")
            fh.write("# id — `PlayerMap.players`' container, since `Entity.hashCode()`\n")
            fh.write("# returns `getId()`. The two columns being equal on every row is the\n")
            fh.write("# measurement behind `one walk, two KEYHASH sites`, and the check\n")
            fh.write("# refuses on any row where they differ. Replayed by\n")
            fh.write("# crates/rustmc-server/src/openhash.rs's test of the same name.\n")
            for ops, walk, obj in rows:
                fh.write("%s|%s|%s\n" % (ops, walk, obj))
        print("rewrote %s and %s from a cold probe run" % (PIN, GOLDEN))
        return 0

    if not os.path.exists(PIN):
        die("no pin at %s; run --measure once and commit it" % PIN)
    pin = json.load(open(PIN, encoding="utf-8"))
    if pin.get("fastutil_sha256") != sha256(fu):
        die("the pin was measured against a different fastutil (%s on disk)"
            % sha256(fu)[:16])
    header, rows = read_golden()
    if header.get("fastutil_sha256") != sha256(fu):
        die("the walk golden was measured against a different fastutil (%s "
            "on disk) — re-measure rather than replaying it"
            % sha256(fu)[:16])

    if args.json:
        print(json.dumps({"inputs": facts, "users": users, "ours": ours,
                          "golden_histories": len(rows)},
                         indent=2, sort_keys=True))
        return 0

    print("the ten inputs to Int2ObjectOpenHashMap's walk order, from %s"
          % FASTUTIL_REL.rsplit("/", 1)[-1])
    print()
    for k in sorted(facts, key=lambda s: int(s.split("_", 1)[0])):
        v = facts[k]
        print("  %-22s %s" % (k.split("_", 1)[1],
                              ", ".join("%s=%s" % kv for kv in sorted(v.items()))))
    print()
    for k, v in sorted(users.items()):
        print("  %-22s %s" % (k, v))
    print()
    print("  ours: %s, citing all ten" % ours["source"])
    permuted = sum(1 for _, w in rows if w != sorted(w))
    with_removes = sum(1 for ops, _ in rows if any(o[0] == "-" for o in ops))
    print("  golden: %d measured histories, %d walking a real permutation, "
          "%d containing a remove" % (len(rows), permuted, with_removes))

    if args.check:
        if facts != pin["inputs"]:
            print("fastutil-open-hash-walk: an input moved against its pin",
                  file=sys.stderr)
            for k in sorted(facts):
                if pin["inputs"].get(k) != facts[k]:
                    print("   %s: pinned %r, now %r"
                          % (k, pin["inputs"].get(k), facts[k]), file=sys.stderr)
            return 1
        if users != pin["users"]:
            print("fastutil-open-hash-walk: a consumer moved against its pin: "
                  "pinned %r, now %r" % (pin["users"], users), file=sys.stderr)
            return 1
        print("fastutil-open-hash-walk: 10 inputs and 2 consumers, exactly as pinned")
    return 0


if __name__ == "__main__":
    sys.exit(main())
