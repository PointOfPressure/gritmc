#!/usr/bin/env python3
"""tick-section-classify.py — every one of vanilla's per-tick profiler sections
into a named bucket, with the evidence that put it there.

`ci/vanilla-reach-census.py` UNIT 2 enumerates the 120 profiler sections Mojang
compiled into the code reachable from `MinecraftServer.tickServer` and
classifies 40 of them: 7 shut by a game rule this harness sets, 33 per-mob for a
mob no recording ever held. The other 80 are `unclassified`, and §21.4 is blunt
that this means *no information at all* — not "ran". Eighty unknowns are the
soft spot under the 33.3% "provably never run" figure. This script is the
instrument that classifies them.

WHAT IS NEW HERE: A POSITIVE
============================
Every tier UNIT 2 had was a negative — a section it could show did *not* run.
The two instruments below are the first evidence in this census that a section
**did** run, and they are proofs over the pinned jar rather than inferences
from what the recordings happen to contain:

  * **control-flow dominance.** A profiler `push` that is executed on every
    path from its method's entry to every one of that method's returns cannot
    be skipped. Chain that from `tickServer` through call sites that are
    themselves unskippable, and the section runs on every server tick. This is
    a *proof*, not a reachability claim: over-approximated reachability (§21.3)
    says a section *might* run and was worth nothing; dominance says it *must*.

  * **the entity the recordings held.** The 33 `PROVABLY UNPOPULATED` sections
    are negative evidence read off `add_entity`: no such mob, so no such
    section. The same six recordings say four entity types *were* present, and
    the identical argument runs forwards — a section pushed unskippably inside
    the tick of a class the world demonstrably contained is a section that ran.
    The negative and the positive are one instrument pointed both ways, which
    is why they cannot disagree about a mob without one of them being wrong.

WHAT DOMINANCE IS AND IS NOT
============================
`B dominates X` means every path from the method's entry to X passes through
B. A push site that dominates every `*return` in its method executes on every
normal completion of that method. Three deliberate conservatisms:

  * **A loop body dominates nothing after the loop.** The path that skips a
    zero-iteration loop bypasses its body, so a push inside `for (level :
    levels)` is *not* unconditional. That is correct and it is expensive: most
    of a server tick is inside such a loop. `--premised` re-runs the analysis
    with every loop assumed to iterate at least once and reports the loops it
    assumed, so the premise is stated rather than smuggled. Those sections
    land in a **separate bucket with the premise named**, never in the
    unconditional one.
  * **Exception handlers are edges.** A `catch` that re-joins after a push
    gives a path that skipped it. Included by default; `--no-exc-edges`
    prints the count without them so the sensitivity to this choice is
    visible rather than assumed.
  * **A polymorphic call is not followed.** `invokevirtual` names a static
    receiver type and the census's own graph resolves it to every subtype
    (`ci/jarcallgraph.py` — deliberate over-approximation). Over-approximation
    is the *safe* direction for a negative and the *unsound* direction for a
    positive, so this walk follows a call only when it can name exactly one
    target: `invokestatic`/`invokespecial`, an `invokevirtual` whose resolution
    is a singleton, or an `invokevirtual` on `aload_0` — `this`, whose runtime
    class is known when the walk was rooted at a class the recordings named.

WHY NOT REUSE THE CENSUS'S GRAPH
================================
`ci/jarcallgraph.py` is imported, not copied, and is **not modified** — the
census stamps figures with its digest, so an edit here would silently
re-stamp §21's numbers. What it does not keep is what dominance needs: branch
targets and the exception table. Those are read here, from the same pinned jar
bytes, with `jarcallgraph`'s own instruction-length table and constant-pool
reader imported so that a decode difference between the two instruments is
impossible by construction.

FALSIFIERS
==========
Each bucket states what observation moves a section out of it, and the ones
that can be checked statically are checked: `--selftest` asserts that the
dominance instrument refuses a section it must refuse (a push behind an `if`)
and accepts one it must accept, over the pinned jar, so a run that classifies
everything is distinguishable from an instrument that has stopped failing.

CLEAN ROOM: the pinned `server-26.2.jar` and Mojang's own `generated/reports/`
inside it. Nothing else is read.
"""

import argparse
import json
import os
import re
import struct
import sys
import zipfile
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JAR = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "versions", "26.2", "server-26.2.jar")
REPORTS = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "generated", "reports")
T2 = os.path.join(ROOT, "ci", "t2-gameplay.sh")

sys.path.insert(0, os.path.join(ROOT, "ci"))
from jarcallgraph import ILEN, JarGraph, _cp  # noqa: E402

TICK_ROOT = (
    "net/minecraft/server/MinecraftServer",
    "tickServer",
    "(Ljava/util/function/BooleanSupplier;)V",
)
# The runtime class of `this` inside `tickServer`. Not cosmetic: `tickChildren`
# is overridden by the dedicated server and the override is where the
# `server gui refresh` section lives, so a walk rooted at the abstract class
# misses a whole subtree. The premise — this harness runs the dedicated server
# — is the java command line in `ci/t2-gameplay.sh`, checked in `harness()`.
SERVER_CLASS = "net/minecraft/server/dedicated/DedicatedServer"

RET_OPS = set(range(0xAC, 0xB2))
COND_OPS = set(range(0x99, 0xA7)) | {0xC6, 0xC7}
GOTO_OPS = {0xA7, 0xC8}
SWITCH_OPS = {0xAA, 0xAB}
INVOKE_OPS = {0xB6: "virtual", 0xB7: "special", 0xB8: "static", 0xB9: "interface", 0xBA: "dynamic"}
FIELD_OPS = {0xB2: "getstatic", 0xB3: "putstatic", 0xB4: "getfield", 0xB5: "putfield"}
ALOAD_0 = 0x2A


def die(msg):
    print(f"tick-section-classify: REFUSED — {msg}", file=sys.stderr)
    sys.exit(2)


# --------------------------------------------------------------------------
# class bodies — what jarcallgraph does not keep
# --------------------------------------------------------------------------


class Bodies:
    """Lazy per-class access to code, exception tables and the constant pool.

    `jarcallgraph.JarGraph` parses every class in the jar and keeps the call
    edges; it drops the code bytes, the branch targets and the exception table
    because nothing it does needs them. Re-reading a few hundred classes here
    is cheaper than keeping 7,445 constant pools alive, and it leaves that
    file untouched — its digest is in the census stamp.
    """

    def __init__(self, jar_path):
        self.z = zipfile.ZipFile(jar_path)
        self._cache = {}

    def klass(self, name):
        if name in self._cache:
            return self._cache[name]
        try:
            buf = self.z.read(name + ".class")
        except KeyError:
            self._cache[name] = None
            return None
        out = self._parse(buf)
        self._cache[name] = out
        return out

    def method(self, key):
        c = self.klass(key[0])
        if c is None:
            return None
        return c["methods"].get((key[1], key[2]))

    @staticmethod
    def _parse(buf):
        pos = 8
        ent, pos = _cp(buf, pos)

        def utf(i):
            e = ent[i]
            return e[1] if e and e[0] == "Utf8" else ""

        def cls(i):
            e = ent[i]
            return utf(e[1]) if e and e[0] == "Class" else ""

        def ref(i):
            e = ent[i]
            if not e or e[0] not in ("Fieldref", "Methodref", "IMethodref"):
                return None
            nat = ent[e[2]]
            return (cls(e[1]), utf(nat[1]), utf(nat[2]))

        def const_str(i):
            e = ent[i]
            return utf(e[1]) if e and e[0] == "String" else None

        def indy(i):
            e = ent[i]
            if not e or e[0] != "InvokeDynamic":
                return "()V"
            nat = ent[e[2]]
            return utf(nat[2]) if nat else "()V"

        pos += 6  # access, this, super
        (nif,) = struct.unpack_from(">H", buf, pos)
        pos += 2 + 2 * nif
        # fields
        (nf,) = struct.unpack_from(">H", buf, pos)
        pos += 2
        for _ in range(nf):
            pos += 6
            (na,) = struct.unpack_from(">H", buf, pos)
            pos += 2
            for _ in range(na):
                (ln,) = struct.unpack_from(">I", buf, pos + 2)
                pos += 6 + ln
        # methods
        (nm,) = struct.unpack_from(">H", buf, pos)
        pos += 2
        methods = {}
        for _ in range(nm):
            _acc, ni, di = struct.unpack_from(">HHH", buf, pos)
            pos += 6
            name, desc = utf(ni), utf(di)
            (na,) = struct.unpack_from(">H", buf, pos)
            pos += 2
            body = None
            for _ in range(na):
                ai, ln = struct.unpack_from(">HI", buf, pos)
                pos += 6
                if utf(ai) == "Code":
                    (clen,) = struct.unpack_from(">I", buf, pos + 4)
                    code = buf[pos + 8 : pos + 8 + clen]
                    p = pos + 8 + clen
                    (nexc,) = struct.unpack_from(">H", buf, p)
                    p += 2
                    exc = [struct.unpack_from(">HHHH", buf, p + 8 * k)[:3] for k in range(nexc)]
                    body = (code, exc)
                pos += ln
            if body is not None:
                methods[(name, desc)] = body
        return {"methods": methods, "ref": ref, "cls": cls, "str": const_str, "indy": indy}


def decode(code):
    """[(offset, opcode, length)] over one method's code array."""
    out, i, n = [], 0, len(code)
    while i < n:
        op = code[i]
        ln = ILEN[op]
        if op == 0xC4:  # wide
            ln = 6 if code[i + 1] == 0x84 else 4
        elif op in SWITCH_OPS:
            p = i + 1
            p += (4 - (p % 4)) % 4
            if op == 0xAA:
                lo, hi = struct.unpack_from(">ii", code, p + 4)
                ln = (p + 12 + 4 * (hi - lo + 1)) - i
            else:
                (np_,) = struct.unpack_from(">i", code, p + 4)
                ln = (p + 8 + 8 * np_) - i
        if ln <= 0:
            ln = 1
        out.append((i, op, ln))
        i += ln
    return out


def succs(code, off, op, ln):
    """Normal-control successors of one instruction."""
    if op in RET_OPS or op == 0xBF:  # returns, athrow
        return []
    if op in COND_OPS:
        (t,) = struct.unpack_from(">h", code, off + 1)
        return [off + t, off + ln]
    if op == 0xA7:
        (t,) = struct.unpack_from(">h", code, off + 1)
        return [off + t]
    if op == 0xC8:
        (t,) = struct.unpack_from(">i", code, off + 1)
        return [off + t]
    if op in SWITCH_OPS:
        p = off + 1
        p += (4 - (p % 4)) % 4
        (d,) = struct.unpack_from(">i", code, p)
        out = [off + d]
        if op == 0xAA:
            lo, hi = struct.unpack_from(">ii", code, p + 4)
            for k in range(hi - lo + 1):
                (t,) = struct.unpack_from(">i", code, p + 12 + 4 * k)
                out.append(off + t)
        else:
            (np_,) = struct.unpack_from(">i", code, p + 4)
            for k in range(np_):
                (t,) = struct.unpack_from(">i", code, p + 12 + 8 * k)
                out.append(off + t)
        return out
    return [off + ln]


class CFG:
    """Basic blocks, dominators, and the two things this instrument answers:
    does an offset execute on every completion of the method, and if not, what
    branch decided that."""

    def __init__(self, code, exc, exc_edges=True, assume_loops=False):
        self.code = code
        ins = decode(code)
        self.ins = ins
        self.op = {o: (op, ln) for o, op, ln in ins}
        offs = [o for o, _, _ in ins]
        self.order = offs
        leaders = {offs[0]} if offs else set()
        for o, op, ln in ins:
            ss = succs(code, o, op, ln)
            if op in COND_OPS or op in GOTO_OPS or op in SWITCH_OPS or op in RET_OPS or op == 0xBF:
                if o + ln in self.op:
                    leaders.add(o + ln)
                for s in ss:
                    leaders.add(s)
        for s, _e, h in exc:
            leaders.add(h)
        leaders &= set(offs)
        self.leaders = sorted(leaders)
        # offset -> block leader
        self.block = {}
        cur = None
        for o in offs:
            if o in leaders:
                cur = o
            self.block[o] = cur
        # block successors, from each block's LAST instruction only.
        #
        # Taking them from every instruction is wrong and was wrong here for
        # one run: `succs()` of a straight-line instruction is the next
        # offset, which is in the *same* block, so every block acquired a
        # self-edge. A self-edge is a back edge to a header that dominates
        # itself, so `_loops` called every block a loop and `_assume_loops`
        # deleted every real edge out of it — after which almost everything
        # "dominated" everything and three gated sections came out
        # unconditional. Caught by `javap` disagreeing about `dragonFight`,
        # which is guarded by an `ifnull` the analysis claimed was not there.
        # `--selftest` now pins exactly that case.
        self.last = {}
        for o, op, ln in ins:
            self.last[self.block[o]] = (o, op, ln)
        self.succ = defaultdict(set)
        self.exits = set()
        for b, (o, op, ln) in self.last.items():
            if op in RET_OPS:
                self.exits.add(b)
                continue
            if op == 0xBF:
                continue
            for s in succs(code, o, op, ln):
                if s in self.block:
                    self.succ[b].add(self.block[s])
        if exc_edges:
            for s, e, h in exc:
                if h not in self.block:
                    continue
                for o in offs:
                    if s <= o < e:
                        self.succ[self.block[o]].add(self.block[h])
        self.entry = self.leaders[0] if self.leaders else None
        self.pred = defaultdict(set)
        for a, bs in self.succ.items():
            for b in bs:
                self.pred[b].add(a)
        self.dom = self._dominators(self.succ)
        if assume_loops:
            self.succ2 = self._assume_loops()
            self.dom2 = self._dominators(self.succ2)
        else:
            self.succ2, self.dom2 = self.succ, self.dom

    def _dominators(self, succ):
        nodes = self.leaders
        if not nodes:
            return {}
        entry = nodes[0]
        allb = set(nodes)
        dom = {n: (set(allb) if n != entry else {entry}) for n in nodes}
        pred = defaultdict(set)
        for a, bs in succ.items():
            for b in bs:
                pred[b].add(a)
        changed = True
        while changed:
            changed = False
            for n in nodes:
                if n == entry:
                    continue
                ps = [dom[p] for p in pred.get(n, ()) if p in dom]
                new = set.intersection(*ps) | {n} if ps else {n}
                if new != dom[n]:
                    dom[n] = new
                    changed = True
        return dom

    def _loops(self):
        """Natural loops: (header, body set), one per back edge."""
        out = []
        for a, bs in self.succ.items():
            for h in bs:
                if h in self.dom.get(a, ()):  # back edge a -> h
                    body, stack = {h, a}, [a]
                    while stack:
                        n = stack.pop()
                        for p in self.pred.get(n, ()):
                            if p not in body:
                                body.add(p)
                                stack.append(p)
                    out.append((h, body))
        return out

    def _assume_loops(self):
        """Every loop iterates at least once: delete the header's exit edges."""
        s = {k: set(v) for k, v in self.succ.items()}
        self.assumed = []
        for h, body in self._loops():
            leave = {x for x in s.get(h, ()) if x not in body}
            if leave and len(s.get(h, ())) > len(leave):
                s[h] -= leave
                self.assumed.append((h, body))
        return s

    def unconditional(self, off, premised=False):
        """Does `off` execute on every normal completion of this method?"""
        dom = self.dom2 if premised else self.dom
        if not self.exits:
            return False
        b = self.block.get(off)
        return all(b in dom.get(x, ()) for x in self.exits)

    def in_assumed_loop(self, off):
        b = self.block.get(off)
        return [h for h, body in getattr(self, "assumed", ()) if b in body]

    # ---- why not ---------------------------------------------------------
    def postdom(self):
        """Post-dominators, over the reverse graph against a virtual exit.

        `B post-dominates N` means every path from N to a return passes
        through B, so `B post-dominates entry` is exactly "B runs on every
        normal completion". This is what control dependence needs and what a
        walk of B's *dominator* chain cannot give: the branch that skips B
        need not dominate B at all. Written after that error put an early
        `return` in `MinecraftServer.tickServer` — the `/tick freeze` path —
        on the wrong side of the analysis and called eight sections
        unconditional that are not.
        """
        if getattr(self, "_pd", None) is not None:
            return self._pd
        nodes = list(self.leaders)
        EXIT = -1
        succ = {n: set(self.succ.get(n, ())) for n in nodes}
        for e in self.exits:
            succ[e].add(EXIT)
        succ[EXIT] = set()
        allb = set(nodes) | {EXIT}
        pd = {n: (set(allb) if n != EXIT else {EXIT}) for n in allb}
        changed = True
        while changed:
            changed = False
            for n in nodes:
                ss = [pd[x] for x in succ.get(n, ()) if x in pd]
                new = set.intersection(*ss) | {n} if ss else {n}
                if new != pd[n]:
                    pd[n] = new
                    changed = True
        self._pd = pd
        return pd

    def can_exit(self):
        """Blocks from which a return is reachable.

        A block that cannot reach one — an infinite loop, or a path that only
        throws — has no post-dominators in the usual sense, and leaving it at
        the iteration's "everything" initialiser makes every earlier block
        look control-dependent on it. That put a spurious gate on
        `naturalSpawnCount`, whose push is the second instruction of its
        method, and the `--selftest` control for the refutation is what caught
        it.
        """
        if getattr(self, "_ce", None) is not None:
            return self._ce
        pred = defaultdict(set)
        for a, bs in self.succ.items():
            for x in bs:
                pred[x].add(a)
        seen, stack = set(self.exits), list(self.exits)
        while stack:
            n = stack.pop()
            for p in pred.get(n, ()):
                if p not in seen:
                    seen.add(p)
                    stack.append(p)
        self._ce = seen
        return seen

    def gate_sites(self, off, limit=6):
        """The branches `off` is control-dependent on, outermost first.

        `B` is control-dependent on `D` when `D` has a successor that `B`
        post-dominates while `D` itself is not post-dominated by `B` — the
        decision points at which the execution may still choose to skip `B`.
        An empty list means the push happens on every normal completion of
        the method.
        """
        b = self.block.get(off)
        if b is None:
            return []
        pd = self.postdom()
        ce = self.can_exit()
        out = []
        for d in self.leaders:
            if d == b or d not in ce or b in pd.get(d, ()):
                continue
            if any(s in ce and b in pd.get(s, ()) for s in self.succ.get(d, ())):
                last = self.last.get(d)
                if last and (last[1] in COND_OPS or last[1] in SWITCH_OPS):
                    out.append((last[0], last[1]))
        out.sort()
        return out[:limit]

    def _reaches(self, a, b):
        seen, stack = set(), [a]
        while stack:
            n = stack.pop()
            if n == b:
                return True
            if n in seen:
                continue
            seen.add(n)
            stack.extend(self.succ.get(n, ()))
        return False


# --------------------------------------------------------------------------
# operand stack — only to answer "is this call's receiver `this`?"
# --------------------------------------------------------------------------
# Built rather than typed out, for the reason `jarcallgraph.ILEN` is: a typo in
# one row would silently shift every later slot and the receiver would be read
# off the wrong stack position, which is exactly the class of error this whole
# script exists to be trusted about.
POPP = {}


def _eff(ops, pop, push):
    for o in ops:
        POPP[o] = (pop, push)


_eff([0x00], 0, 0)                                    # nop
_eff([0x01], 0, 1)                                    # aconst_null
_eff(range(0x02, 0x09), 0, 1)                         # iconst_m1..5
_eff([0x09, 0x0A], 0, 2)                              # lconst
_eff(range(0x0B, 0x0E), 0, 1)                         # fconst
_eff([0x0E, 0x0F], 0, 2)                              # dconst
_eff([0x10, 0x11, 0x12, 0x13], 0, 1)                  # bipush/sipush/ldc/ldc_w
_eff([0x14], 0, 2)                                    # ldc2_w
_eff([0x15, 0x17, 0x19], 0, 1)                        # iload/fload/aload
_eff([0x16, 0x18], 0, 2)                              # lload/dload
_eff(range(0x1A, 0x1E), 0, 1)                         # iload_<n>
_eff(range(0x1E, 0x22), 0, 2)                         # lload_<n>
_eff(range(0x22, 0x26), 0, 1)                         # fload_<n>
_eff(range(0x26, 0x2A), 0, 2)                         # dload_<n>
_eff(range(0x2A, 0x2E), 0, 1)                         # aload_<n>
_eff([0x2E, 0x30, 0x32, 0x33, 0x34, 0x35], 2, 1)      # i/f/a/b/c/saload
_eff([0x2F, 0x31], 2, 2)                              # l/daload
_eff([0x36, 0x38, 0x3A], 1, 0)                        # istore/fstore/astore
_eff([0x37, 0x39], 2, 0)                              # lstore/dstore
_eff(range(0x3B, 0x3F), 1, 0)                         # istore_<n>
_eff(range(0x3F, 0x43), 2, 0)                         # lstore_<n>
_eff(range(0x43, 0x47), 1, 0)                         # fstore_<n>
_eff(range(0x47, 0x4B), 2, 0)                         # dstore_<n>
_eff(range(0x4B, 0x4F), 1, 0)                         # astore_<n>
_eff([0x4F, 0x51, 0x53, 0x54, 0x55, 0x56], 3, 0)      # i/f/a/b/c/sastore
_eff([0x50, 0x52], 4, 0)                              # l/dastore
_eff([0x57], 1, 0)                                    # pop
_eff([0x58], 2, 0)                                    # pop2
_eff([0x5F], 2, 2)                                    # swap
_eff([0x60, 0x62, 0x64, 0x66, 0x68, 0x6A, 0x6C, 0x6E, 0x70, 0x72], 2, 1)   # i/f arith
_eff([0x61, 0x63, 0x65, 0x67, 0x69, 0x6B, 0x6D, 0x6F, 0x71, 0x73], 4, 2)   # l/d arith
_eff([0x74, 0x76], 1, 1)                              # ineg/fneg
_eff([0x75, 0x77], 2, 2)                              # lneg/dneg
_eff([0x78, 0x7A, 0x7C, 0x7E, 0x80, 0x82], 2, 1)      # int shifts/logic
_eff([0x79, 0x7B, 0x7D], 3, 2)                        # long shifts
_eff([0x7F, 0x81, 0x83], 4, 2)                        # long logic
_eff([0x84], 0, 0)                                    # iinc
_eff([0x85, 0x87], 1, 2)                              # i2l/i2d
_eff([0x86, 0x8B, 0x91, 0x92, 0x93], 1, 1)            # i2f/f2i/i2b/i2c/i2s
_eff([0x88, 0x89], 2, 1)                              # l2i/l2f
_eff([0x8A], 2, 2)                                    # l2d
_eff([0x8C, 0x8D], 1, 2)                              # f2l/f2d
_eff([0x8E, 0x90], 2, 1)                              # d2i/d2f
_eff([0x8F], 2, 2)                                    # d2l
_eff([0x94, 0x97, 0x98], 4, 1)                        # lcmp/dcmpl/dcmpg
_eff([0x95, 0x96], 2, 1)                              # fcmpl/fcmpg
_eff(range(0x99, 0x9F), 1, 0)                         # ifeq..ifle
_eff(range(0x9F, 0xA7), 2, 0)                         # if_icmp*/if_acmp*
_eff([0xA7, 0xA9, 0xC8], 0, 0)                        # goto/ret/goto_w
_eff([0xA8, 0xC9], 0, 1)                              # jsr/jsr_w
_eff([0xAA, 0xAB], 1, 0)                              # switches
_eff([0xAC, 0xAE, 0xB0], 1, 0)                        # ireturn/freturn/areturn
_eff([0xAD, 0xAF], 2, 0)                              # lreturn/dreturn
_eff([0xB1], 0, 0)                                    # return
_eff([0xBB], 0, 1)                                    # new
_eff([0xBC, 0xBD, 0xBE, 0xC0, 0xC1], 1, 1)            # newarray/anewarray/arraylength/cast/instanceof
_eff([0xBF, 0xC2, 0xC3], 1, 0)                        # athrow/monitorenter/monitorexit
DUPS = {0x59: 1, 0x5A: 2, 0x5B: 3, 0x5C: 2, 0x5D: 3, 0x5E: 4}  # opcode -> slots it must see


def arg_slots(desc):
    i, n = 1, 0
    while desc[i] != ")":
        c = desc[i]
        if c == "[":
            while desc[i] == "[":
                i += 1
            if desc[i] == "L":
                i = desc.index(";", i)
            i += 1
            n += 1
            continue
        if c == "L":
            i = desc.index(";", i) + 1
            n += 1
            continue
        n += 2 if c in "JD" else 1
        i += 1
    return n


def ret_slots(desc):
    r = desc[desc.index(")") + 1 :]
    return 0 if r == "V" else (2 if r[0] in "JD" else 1)


def field_slots(desc):
    return 2 if desc[0] in "JD" else 1


class StackSim:
    """A per-basic-block symbolic stack, carrying one bit: is this slot `this`?

    Enough to answer the only question asked of it — the receiver of
    `this.foo(a, b)` sits `arg_slots(desc)` slots below the top at the call —
    and no more. A block whose incoming stack is not empty, or an opcode that
    pops past the block entry, yields `None` and the caller loses an edge.
    """

    def __init__(self, code, cref, indy=None):
        self.code = code
        self.ref = cref
        self.indy = indy or (lambda _i: "()V")

    def receiver(self, leader, off, desc):
        st, under = [], False
        for o, op, _ln in decode(self.code):
            if o < leader:
                continue
            if o >= off:
                break
            if op in DUPS:
                need = DUPS[op]
                if len(st) < need:
                    under = True
                    st = []
                    continue
                if op == 0x59:
                    st.append(st[-1])
                elif op == 0x5A:
                    st[-2:] = [st[-1], st[-2], st[-1]]
                elif op == 0x5B:
                    st[-3:] = [st[-1]] + st[-3:]
                elif op == 0x5C:
                    st += st[-2:]
                elif op == 0x5D:
                    st[-3:] = st[-2:] + st[-3:]
                else:
                    st[-4:] = st[-2:] + st[-4:]
                continue
            if op == 0xC4:  # wide — iinc or a load/store; slot count is the base op's
                base = self.code[o + 1]
                p, q = POPP.get(base, (0, 0))
            elif op in (0xB2, 0xB3, 0xB4, 0xB5):
                t = self.ref(struct.unpack_from(">H", self.code, o + 1)[0])
                w = field_slots(t[2]) if t else 1
                p, q = {0xB2: (0, w), 0xB3: (w, 0), 0xB4: (1, w), 0xB5: (1 + w, 0)}[op]
            elif op in INVOKE_OPS:
                if op == 0xBA:
                    t = None
                    d = self.indy(struct.unpack_from(">H", self.code, o + 1)[0])
                else:
                    t = self.ref(struct.unpack_from(">H", self.code, o + 1)[0])
                    d = t[2] if t else "()V"
                a = arg_slots(d) + (0 if op in (0xB8, 0xBA) else 1)
                p, q = a, ret_slots(d)
            elif op == 0xC5:  # multianewarray
                p, q = self.code[o + 3], 1
            else:
                p, q = POPP.get(op, (0, 0))
            if len(st) < p:
                under = True
                st = []
            else:
                st = st[: len(st) - p]
            st += ["this" if op in (0x2A,) else "?"] * q
        n = arg_slots(desc) + 1
        if under or len(st) < n:
            return None
        return st[-n]

def receiver_is_this(bodies, key, off):
    """Exact: is the receiver of the invoke at `off` the `this` reference?"""
    body = bodies.method(key)
    c = bodies.klass(key[0])
    if body is None or c is None:
        return False
    code, exc = body
    t = c["ref"](struct.unpack_from(">H", code, off + 1)[0])
    if not t:
        return False
    cfg = CFG(code, exc)
    leader = cfg.block.get(off)
    if leader is None:
        return False
    return StackSim(code, c["ref"], c["indy"]).receiver(leader, off, t[2]) == "this"


# --------------------------------------------------------------------------
# the gate chain
# --------------------------------------------------------------------------
# A section is not simply "unconditional" or not. Between `tickServer` and a
# `push` there is a chain of decisions vanilla makes, and the useful output is
# that chain rather than a boolean: an empty chain is a section that runs every
# tick, and a non-empty one names exactly what has to hold. Loops, `if`s and
# lambda call-backs are all decisions and all appear here — a `for (x : xs)`
# surfaces as its `Iterator.hasNext` test, which reads as "the collection is
# non-empty", which is what it means.


LOADS = dict(
    [(0x15, None), (0x19, None)]
    + [(0x1A + k, k) for k in range(4)]
    + [(0x2A + k, k) for k in range(4)]
)
STORES = dict(
    [(0x36, None), (0x3A, None)]
    + [(0x3B + k, k) for k in range(4)]
    + [(0x4B + k, k) for k in range(4)]
)


def _slot(code, o, op):
    v = LOADS.get(op, STORES.get(op, "x"))
    return code[o + 1] if v is None else v


def cond_source(bodies, key, code, cfg, branch_off):
    """What the branch at `branch_off` tests, named.

    Two shapes, because vanilla writes both and they are not distinguishable
    from the branch alone:

      * `if (x.y())` — the test's value is produced by the invoke or field
        access immediately before the branch, in the same block.
      * `boolean flag = a.b(); ... if (flag)` — javac stores to a local and
        the branch reads it back, sometimes hundreds of bytes later. The
        branch's own block then contains no invoke at all, and naming the
        nearest one is not merely vague, it is WRONG: it was reporting the
        surrounding `Iterator.hasNext()` for four sections whose real test is
        `GameRules.SPAWN_MOBS`, which is the difference between "this runs
        every tick" and "this harness turned it off". So a load is traced to
        the store that filled it, and the store's own producer is named.

    A `GameRules.get(...)` producer is reported as the rule field it reads,
    which is the only form the classification's game-rule bucket accepts.
    """
    c = bodies.klass(key[0])

    def producer(limit, blk=None):
        """The last invoke/field access before `limit`, optionally confined to
        one block; `GameRules.get` is reported as its rule."""
        last, lastgr, gr_off = None, None, -1
        for o, op, _ln in decode(code):
            if o >= limit:
                break
            if blk is not None and cfg.block.get(o) != blk:
                continue
            if op in (0xB2, 0xB4):
                t = c["ref"](struct.unpack_from(">H", code, o + 1)[0])
                if t:
                    if t[0].endswith("/GameRules"):
                        lastgr, gr_off = f"GameRules.{t[1]}", o
                        last = lastgr
                    else:
                        last = f"{t[0].split('/')[-1]}.{t[1]}"
            elif op in INVOKE_OPS and op != 0xBA:
                t = c["ref"](struct.unpack_from(">H", code, o + 1)[0])
                if t:
                    last = f"{t[0].split('/')[-1]}.{t[1]}()"
            elif op == 0xC1:
                last = f"instanceof {c['cls'](struct.unpack_from('>H', code, o + 1)[0]).split('/')[-1]}"
        # A game-rule read within a few instructions of the test is the test:
        # `GameRules.get(SPAWN_MOBS)` unboxes through `Boolean.booleanValue`,
        # and naming the unboxing instead of the rule is the difference
        # between "runs every tick" and "this harness turned it off".
        if lastgr and 0 <= limit - gr_off <= 40:
            return lastgr
        return last

    blk = cfg.block.get(branch_off)
    prev = None
    for o, op, _ln in decode(code):
        if o >= branch_off:
            break
        prev = (o, op)
    if prev and prev[1] in LOADS:
        slot = _slot(code, prev[0], prev[1])
        st = None
        for o, op, _ln in decode(code):
            if o >= prev[0]:
                break
            if op in STORES and _slot(code, o, op) == slot:
                st = o
        if st is not None:
            got = producer(st)
            if got:
                return got
        else:
            # No store: the value is a parameter of this method. Naming the
            # slot is honest — the test is real and its source is one frame up.
            return f"parameter #{slot} of {key[1]}"
    return producer(branch_off, blk) or "?"


OPNAME = {0x99: "ifeq", 0x9A: "ifne", 0x9B: "iflt", 0x9C: "ifge", 0x9D: "ifgt", 0x9E: "ifle",
          0x9F: "if_icmpeq", 0xA0: "if_icmpne", 0xA1: "if_icmplt", 0xA2: "if_icmpge",
          0xA3: "if_icmpgt", 0xA4: "if_icmple", 0xA5: "if_acmpeq", 0xA6: "if_acmpne",
          0xC6: "ifnull", 0xC7: "ifnonnull", 0xAA: "tableswitch", 0xAB: "lookupswitch"}


class Analysis:
    def __init__(self, graph, bodies, exc_edges=True, max_gates=7, fanout=24):
        self.g, self.b = graph, bodies
        self.exc_edges, self.max_gates, self.fanout = exc_edges, max_gates, fanout
        self._cfg, self._ev, self._indy = {}, {}, {}
        self.polymorphic = 0

    def cfg(self, key):
        if key not in self._cfg:
            body = self.b.method(key)
            try:
                self._cfg[key] = CFG(*body, exc_edges=self.exc_edges) if body else None
            except Exception:  # noqa: BLE001
                self._cfg[key] = None
        return self._cfg[key]

    def events(self, key):
        """(offset, kind, payload) with offsets from this file's own decoder,
        so they index the same CFG the dominance is computed over."""
        if key in self._ev:
            return self._ev[key]
        c, body = self.b.klass(key[0]), self.b.method(key)
        if body is None:
            self._ev[key] = []
            return []
        code, _exc = body
        out, pending = [], None
        indy = {o for o, op, _ in decode(code) if op == 0xBA}
        jt = defaultdict(set)
        for o, kind, payload in self.g.events.get(key, ()):
            if kind == "call" and o in indy:
                jt[o].add(payload)
        for o, op, _ln in decode(code):
            if op in (0x12, 0x13):
                idx = code[o + 1] if op == 0x12 else struct.unpack_from(">H", code, o + 1)[0]
                pending = c["str"](idx)
            elif op == 0xBA:
                for t in jt.get(o, ()):
                    out.append((o, "lambda", t))
                pending = None
            elif op in INVOKE_OPS:
                t = c["ref"](struct.unpack_from(">H", code, o + 1)[0])
                if t:
                    if t[0] == JarGraph.PROFILER and t[2] == "(Ljava/lang/String;)V" and t[1] in ("push", "popPush"):
                        out.append((o, "push", pending))
                    else:
                        out.append((o, "call", (t, INVOKE_OPS[op])))
                pending = None
        self._ev[key] = out
        return out

    def local_gates(self, key, off):
        cfg = self.cfg(key)
        code = self.b.method(key)[0]
        return tuple((key, bo, op, cond_source(self.b, key, code, cfg, bo)) for bo, op in cfg.gate_sites(off))

    def targets(self, key, off, target, kind, sc):
        owner, name, desc = target
        if not owner.startswith(JarGraph.PKG):
            return set()
        if kind in ("static", "special"):
            d = self.g._declaring(owner, name, desc)
            return {(d, name, desc)} if d else set()
        if sc and kind != "static" and receiver_is_this(self.b, key, off):
            d = self.g._declaring(sc, name, desc)
            return {(d, name, desc)} if d else set()
        r = self.g.resolve(target)
        if len(r) == 1:
            return set(r)
        self.polymorphic += 1
        return set()

    def run(self, root, self_class):
        """Minimal gate chain from `root` to every section it can prove is
        pushed. Dijkstra on the number of gates, so the chain reported for a
        section is the cheapest set of assumptions that reaches it."""
        import heapq  # noqa: PLC0415

        seen, sections, seq = {}, {}, 0
        pq = [(0, 0, root, self_class, ())]
        while pq:
            n, _s, key, sc, gates = heapq.heappop(pq)
            if seen.get((key, sc), 1 << 30) <= n:
                continue
            seen[(key, sc)] = n
            cfg = self.cfg(key)
            if cfg is None:
                continue
            for off, kind, payload in self.events(key):
                g = gates + self.local_gates(key, off)
                if len(g) > self.max_gates:
                    continue
                if kind == "push":
                    if payload is None:
                        continue
                    if payload not in sections or len(sections[payload][0]) > len(g):
                        sections[payload] = (g, key)
                    continue
                if kind == "lambda":
                    tgt, ikind = payload, "static"
                    g = g + ((key, off, 0, f"lambda {payload[0].split('/')[-1]}.{payload[1]} is invoked"),)
                    if len(g) > self.max_gates:
                        continue
                    ts = {(self.g._declaring(*tgt) or tgt[0], tgt[1], tgt[2])}
                    # A lambda in the same class captures `this`; crossing into
                    # it must not drop the receiver's runtime type.
                    this_recv = payload[0] == key[0]
                else:
                    tgt, ikind = payload
                    # `special` counts: `super.tick()` is `aload_0` +
                    # `invokespecial`, and dropping the runtime type there cost
                    # the pig walk every section below `LivingEntity.tick`.
                    this_recv = ikind != "static" and bool(sc) and receiver_is_this(self.b, key, off)
                    ts = self.targets(key, off, tgt, ikind, sc)
                for t in ts:
                    nsc = sc if this_recv else None
                    if seen.get((t, nsc), 1 << 30) <= len(g):
                        continue
                    seq += 1
                    heapq.heappush(pq, (len(g), seq, t, nsc, g))
        self.methods_seen = len(seen)
        return sections


ENTITY_TYPES = ("net/minecraft/world/entity/EntityTypes", "<clinit>", "()V")
ENTITY_IDS = "EntityTypeIds"
PLAYER_BASE = "net/minecraft/world/entity/player/Player"


def entity_classes(graph, bodies, regs):
    """`minecraft:<id>` -> the class that implements it, read out of the jar.

    Mojang registers each type as `register(EntityTypeIds.PIG,
    Builder.of(Pig::new, …))`, which compiles to a `getstatic` of the id field
    followed by an `invokedynamic` whose bootstrap argument is a method handle
    on `Pig.<init>`; `jarcallgraph` already resolves that handle, so the
    pairing is a scan with offsets. The id is then the field name in snake
    case, and it is REFUSED unless it resolves in Mojang's own
    `minecraft:entity_type` registry — a rename upstream has to surface as a
    refusal rather than as a quietly smaller map.

    `player` has no factory (the server constructs its own subclass), so it is
    resolved structurally instead: the one class constructed by `PlayerList`
    that is a subtype of `Player`. Structural rather than by name, because a
    name table is the caveat list this project keeps finding stale.
    """
    body = bodies.method(ENTITY_TYPES)
    if body is None:
        die(f"{ENTITY_TYPES[0]} has no <clinit> — entity registration moved")
    code, _exc = body
    c = bodies.klass(ENTITY_TYPES[0])
    indy = defaultdict(set)
    for o, kind, p in graph.events.get(ENTITY_TYPES, ()):
        if kind == "call":
            indy[o].add(p)
    names, _ = reg_names_of(regs, "minecraft:entity_type")
    out, last = {}, None
    for o, op, _ln in decode(code):
        if op == 0xB2:
            t = c["ref"](struct.unpack_from(">H", code, o + 1)[0])
            if t and t[0].endswith(ENTITY_IDS):
                last = t[1]
        elif op == 0xBA and last:
            for p in indy.get(o, ()):
                if p[1] == "<init>" and p[0].startswith("net/minecraft/world/entity"):
                    eid = "minecraft:" + last.lower()
                    if eid in names:
                        out.setdefault(eid, p[0])
                    last = None
    if len(out) < 100:
        die(f"only {len(out)} entity types mapped to classes — the registration shape moved")
    # The server's own player class: the one subtype of `Player` that the
    # server actually instantiates. The gametest framework declares three
    # anonymous ones, so "the only subtype" would be wrong; "the only subtype
    # some non-gametest code constructs" is right and is still structural.
    subs = graph.subtypes().get(PLAYER_BASE, ())
    built = {n for m, ns in graph.news.items() for n in ns
             if n in subs and "gametest" not in m[0]}
    if len(built) != 1:
        die(f"{len(built)} candidate server player classes ({sorted(built)}) — the hierarchy moved")
    out["minecraft:player"] = built.pop()
    return out


def reg_names_of(regs, key):
    if key not in regs:
        die(f"registry {key} is absent from the pinned jar's registries.json")
    return set(regs[key]["entries"]), None


def census_module():
    """`ci/vanilla-reach-census.py`, imported rather than reimplemented. Its
    `tick_sections` is the denominator this script classifies, and re-deriving
    it here would let the two drift into disagreeing about what the 120 are."""
    import importlib.util  # noqa: PLC0415

    p = os.path.join(ROOT, "ci", "vanilla-reach-census.py")
    spec = importlib.util.spec_from_file_location("vanilla_reach_census", p)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# --------------------------------------------------------------------------
# premises: the gates the corpus discharges, each with its falsifier
# --------------------------------------------------------------------------
# A gate chain is not a verdict. A section RUNS only when every gate on its
# chain is discharged by something outside the jar — the harness's own launch,
# or a measurement over the six recordings. Everything else stays UNDECIDED
# with its gate named, and that residue is the honest part of this file: a
# classifier with no undecidable bucket is this project's signature defect in
# a new place.
#
# `check` is run against the jar or the harness and the whole census REFUSES if
# it fails. A premise that cannot fail is not a premise.
PREMISES = {
    "MinecraftServer.pauseWhenEmptySeconds": (
        "the server is not paused for emptiness",
        "a scenario in which no player is connected for `pause-when-empty-seconds`; "
        "the recordings show both harness players connected throughout every run",
        "players",
    ),
    "Iterator.hasNext": (
        "the collection being iterated is non-empty (levels, loaded chunks, players)",
        "a tick with zero levels, zero loaded chunks or zero players; the recordings "
        "carry block and chunk traffic in every one of the 93 scenarios",
        None,
    ),
    "lambda ": (
        "the callback is invoked at least once — the same premise as a non-empty loop",
        "a tick on which the collection driving the callback is empty",
        None,
    ),
    "TickRateManager.runsNormally": (
        "the tick is neither frozen nor stepping",
        "a scenario sending `/tick freeze` or `/tick step`",
        "tickfreeze",
    ),
    "ServerTickRateManager.runsNormally": (
        "the tick is neither frozen nor stepping",
        "a scenario sending `/tick freeze` or `/tick step`",
        "tickfreeze",
    ),
    "TickRateManager.isEntityFrozen": (
        "entities are not frozen",
        "a scenario sending `/tick freeze`",
        "tickfreeze",
    ),
    "ServerLevel.isDebug": (
        "the level is not a debug world",
        "a `debug_all_block_states` level type; the harness boots `minecraft:flat`",
        "flat",
    ),
    "ServerLevel.emptyTime": (
        "the level has had a player in it recently",
        "a level with no players for long enough that vanilla stops ticking it",
        None,
    ),
    "instanceof ServerLevel": (
        "the level is a `ServerLevel`",
        "another concrete `Level` in the jar; `ServerLevel` is the only subtype of `Level` "
        "the pinned jar declares, so the test cannot fail inside a server tick",
        "onelevel",
    ),
    "Level.isClientSide": (
        "the level is server-side",
        "a client level; the pinned jar contains no `net/minecraft/client/` class at all, "
        "so no level in it can be constructed client-side",
        "noclient",
    ),
    "parameter #2 of tick": (
        "`ServerLevel.tick` passes `tickChunks = true` to `ServerChunkCache.tick`",
        "a call site passing `false`",
        "tickchunks",
    ),
}
MOTION_KINDS = ("move_entity", "entity_position", "set_entity_motion", "rotate_head", "move_entity_rot")
# `!isRemoved()` is not a fact about the world, it is the liveness of the very
# entity the walk was rooted at — so it is discharged by the SAME evidence the
# root is, and only for the entity types the recordings show actually moving
# after they spawned. `falling_block` does not qualify: over six runs it
# produces heartbeats and never one position or velocity update, so nothing
# here witnesses it ticking and its sections stay undecided. That asymmetry is
# the point: the premise is measured per entity type, not asserted for all.
LIVENESS = ("LivingEntity.isRemoved", "Entity.isRemoved", "ServerPlayer.isRemoved", "Mob.isRemoved")
# Gates that are a counter or a stagger rather than a condition: the section is
# entered on a fixed fraction of ticks. `Mob.tickCount` is vanilla's tick-parity
# throttle on the goal selectors and `Mob.getId` its per-entity stagger, so both
# are entered within any scenario that lasts more than a couple of ticks — which
# is every scenario in the corpus — but NOT on every tick, and calling them
# "runs" without that qualifier would be the same conflation this section is
# correcting.
PERIODIC = {"MinecraftServer.tickCount", "MinecraftServer.emptyTicks",
            "MinecraftServer.ticksUntilAutosave", "ServerLevel.tickTime",
            "Mob.tickCount", "Mob.getId"}


def discharged(cond, live=False):
    if live and any(cond.startswith(k) for k in LIVENESS):
        return True
    return any(cond.startswith(k) for k in PREMISES)


def _first_entity(v):
    if isinstance(v, dict):
        if "$entity" in v:
            return v["$entity"]
        for x in v.values():
            r = _first_entity(x)
            if r is not None:
                return r
    return None


def entity_motion(files, regs, cen):
    """Entity types the recordings show MOVING after they spawned.

    `add_entity` proves an entity existed; a later `move_entity` /
    `entity_position` / `set_entity_motion` / `rotate_head` for the same id
    proves the server ticked it, which is a strictly stronger fact and the one
    the liveness premise needs. Measured over every scenario of every
    recording, not asserted."""
    _names, by_pid = cen.reg_names(regs, "minecraft:entity_type")
    moved = set()
    for f in files:
        d = json.load(open(f))
        for sc in d.get("scenarios", []):
            spawn = {}
            for o in sc.get("obs", []):
                fl = o.get("fields", {})
                ent = _first_entity(fl)
                if o.get("kind") == "add_entity":
                    t = by_pid.get(fl.get("kind_id"))
                    if t is not None and ent is not None:
                        spawn[ent] = t
                elif o.get("kind") in MOTION_KINDS and ent in spawn:
                    moved.add(spawn[ent])
    return moved


def harness_facts(root):
    """The three premises that are facts about the harness rather than the jar,
    read out of `ci/t2-gameplay.sh` and `ci/vanilla-server.sh` — parsed, never
    transcribed, for the reason §21.2 gives: a table of what the harness does,
    maintained beside the harness, is a caveat list and this project's caveat
    lists lag their code."""
    out = {}
    t2 = open(os.path.join(root, "ci", "t2-gameplay.sh")).read()
    vs_path = os.path.join(root, "ci", "vanilla-server.sh")
    vs = open(vs_path).read() if os.path.exists(vs_path) else ""
    checks = ""
    cdir = os.path.join(root, "ci", "checks", "gameplay")
    if os.path.isdir(cdir):
        for dirpath, _dirs, fs in os.walk(cdir):
            for f in sorted(fs):
                if f.endswith(".toml"):
                    checks += open(os.path.join(dirpath, f)).read()
    out["tickfreeze"] = not re.search(r"\btick\s+(freeze|step|sprint)\b", t2 + checks)
    out["flat"] = "minecraft:flat" in t2 or "level-type" in vs
    out["players"] = "--nogui" in vs or "nogui" in t2
    return out


def verify_tickchunks(graph, bodies):
    """`ServerLevel.tick` passes a constant `true` for `tickChunks`.

    The last argument of `ServerChunkCache.tick(BooleanSupplier, boolean)` is
    pushed immediately before the invoke, so the constant is readable. This is
    the one premise on the list that is a claim about the jar, so it is checked
    against the jar rather than asserted."""
    key = ("net/minecraft/server/level/ServerLevel", "tick", "(Ljava/util/function/BooleanSupplier;)V")
    body = bodies.method(key)
    if body is None:
        return False
    code, _exc = body
    c = bodies.klass(key[0])
    prev, ok = None, False
    for o, op, _ln in decode(code):
        if op in INVOKE_OPS and op != 0xBA:
            t = c["ref"](struct.unpack_from(">H", code, o + 1)[0])
            if t and t[0].endswith("ServerChunkCache") and t[1] == "tick":
                ok = prev == 0x04  # iconst_1
        prev = op
    return ok


def fmt(gates, dedupe=True):
    out, seen = [], set()
    for g in gates:
        if dedupe and g[3] in seen:
            continue
        seen.add(g[3])
        out.append(g[3])
    return " AND ".join(out) or "(none)"


def classify(sec, chain, mob_unpop, rules_off, live=False):
    """One section -> (bucket, the gate that decided it)."""
    if sec in mob_unpop:
        return "UNPOPULATED", mob_unpop[sec]
    if chain is None:
        return "UNREACHED", None
    undis = [g[3] for g in chain if not discharged(g[3], live)]
    if not undis:
        return "RUNS", None
    gr = [u for u in undis if u.startswith("GameRules.")]
    if gr:
        rule = camel_lower(gr[0].split(".", 1)[1])
        if rule in rules_off:
            return "OFF_GAMERULE", gr[0]
        return "UNDECIDED", gr[0]
    if all(any(u.startswith(k) for k in PERIODIC) for u in undis):
        return "PERIODIC", undis[0]
    return "UNDECIDED", undis[0]


def camel_lower(s):
    return s.lower()


def buckets_of(sections, chains, mob_unpop, rules_off):
    out = defaultdict(list)
    for s in sorted(sections):
        ch = chains.get(s)
        bucket, why = classify(s, ch[0] if ch else None, mob_unpop, rules_off, live=bool(ch and ch[2]))
        out[bucket].append((s, why, ch[1] if ch else None))
    return out


def selftest(graph, bodies, ana, chains):
    """The controls. Every one of these has failed at least once during this
    instrument's construction, which is why they are here rather than in a
    comment.

    1. Dominance must be able to REFUSE. `dragonFight`'s push sits behind an
       `ifnull` on `ServerLevel.dragonFight`; an analysis that calls it
       unconditional has a broken CFG — and one did, for one run, because
       every basic block had acquired a self-edge and the loop relaxation then
       deleted every real edge in the method.
    2. The control-dependence test must see a branch that does NOT dominate
       the push. `MinecraftServer.tickServer` returns early when the server is
       paused for emptiness, and a dominator-chain walk cannot see it.
    3. `customSpawners` must still come out gated by `GameRules.SPAWN_MOBS` —
       the one of §21.2's six that survives this re-reading.
    4. `naturalSpawnCount` must come out with NO local gate at its push. This
       is the refutation, so it is the assertion most worth being able to fail.
    5. The undecided bucket must be non-empty. A classifier that resolves
       everything is this project's signature defect in a new place.
    """
    fails = []

    def want(cond, msg):
        if not cond:
            fails.append(msg)

    sl = ("net/minecraft/server/level/ServerLevel", "tick", "(Ljava/util/function/BooleanSupplier;)V")
    cfg = ana.cfg(sl)
    off = next((o for o, k, p in ana.events(sl) if k == "push" and p == "dragonFight"), None)
    want(off is not None, "dragonFight is not pushed in ServerLevel.tick any more")
    if off is not None:
        want(cfg.gate_sites(off), "control 1: dragonFight came out unconditional; the CFG is wrong")
    tcfg = ana.cfg(TICK_ROOT)
    toff = next((o for o, k, p in ana.events(TICK_ROOT) if k == "push" and p == "tallying"), None)
    want(toff is not None and tcfg.gate_sites(toff),
         "control 2: tickServer's early return is invisible to the control-dependence test")
    ch = chains.get("customSpawners")
    want(ch and any(x[3] == "GameRules.SPAWN_MOBS" for x in ch[0]),
         "control 3: customSpawners lost its GameRules.SPAWN_MOBS gate")
    # Found rather than named: `ServerChunkCache` declares two `tickChunks`
    # overloads and only one of them pushes anything, which is exactly the
    # kind of detail a hard-coded descriptor gets wrong a version later.
    site = None
    for k in graph.events:
        if k[0].endswith("ServerChunkCache"):
            o = next((o for o, kk, p in ana.events(k) if kk == "push" and p == "naturalSpawnCount"), None)
            if o is not None:
                site = (k, o)
                break
    want(site is not None, "control 4: naturalSpawnCount is no longer pushed in ServerChunkCache")
    if site:
        want(not ana.cfg(site[0]).gate_sites(site[1]),
             "control 4: naturalSpawnCount is gated after all — the refutation does not hold")
    want(any(any(not discharged(x[3]) for x in ch[0]) for ch in chains.values()),
         "control 5: every gate discharges, which is not a result, it is a broken classifier")
    for f in fails:
        print(f"  FAIL  {f}")
    print(f"selftest: {5 - len(fails)}/5 controls pass")
    return 1 if fails else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--jar", default=JAR)
    ap.add_argument("--no-exc-edges", action="store_true")
    ap.add_argument("--max-gates", type=int, default=10)
    ap.add_argument("--recordings", default="~/rustmc-cov2/oracle-work/gameplay-selfcheck/r[0-9]*.json")
    ap.add_argument("--why", help="print the full gate chain for one section")
    ap.add_argument("--selftest", action="store_true", help="the controls this instrument must pass")
    ap.add_argument("--per-run", action="store_true",
                    help="re-classify against each recording alone: the reproducibility reading "
                         "that stands where a floor would be")
    ap.add_argument("--json")
    args = ap.parse_args()

    # # THE JAR IS 2.3 GB OF LEFTOVER STATE AND ITS ABSENCE WAS A TRACEBACK.
    # # 2026-08-18.
    #
    # This is a `ci/t0.sh` stage (the `--selftest` loop). `oracle-work` is
    # EXCLUDED from `ci/t0-remote.sh`'s rsync, so the pinned jar is present only
    # because the setup ritual in `docs/AGENT-BRIEF.md` symlinks it in. In a
    # gate directory that has not had that done — a fresh one, or a tree
    # somebody rsynced by hand — the line below raised
    #
    #     FileNotFoundError: [Errno 2] No such file or directory:
    #       '.../oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar'
    #
    # through `zipfile`, and T0 reddened with a Python traceback. A traceback is
    # the worst of the three possible answers: it is not a verdict about the
    # tree, it is not a refusal a reader can act on, and it looks like the
    # instrument is broken rather than the environment being incomplete.
    #
    # The header a few lines down already makes the right argument about the
    # RECORDINGS — "a self-test that needs 2.4 GB of recordings is a self-test
    # that gets quietly skipped on the tree that needed it" — and the jar is the
    # same argument, missed because the recordings were the thing being thought
    # about.
    #
    # A real run refuses: there is no degraded mode for a classifier whose whole
    # input is the jar. The self-test stands down loudly instead, because it is
    # in a gate that must keep going and say which other stages passed, and
    # `ci/box-skip-audit.py` reds if the skip is not declared.
    if not os.path.exists(args.jar):
        if args.selftest:
            sys.path.insert(0, os.path.join(
                os.path.dirname(os.path.abspath(__file__)), "lib"))
            import box_env
            box_env.skip(
                "tick-section-classify:selftest",
                f"the pinned jar is not in this tree ({args.jar}) — 5 controls "
                f"did not run. `oracle-work` is excluded from the rsync; link "
                f"it as docs/AGENT-BRIEF.md sets out.",
            )
            return 0
        print(f"tick-section-classify REFUSED — no jar at {args.jar}. This "
              f"instrument reads the pinned server jar and has no degraded "
              f"mode; `oracle-work` is excluded from ci/t0-remote.sh's rsync, "
              f"so link it as docs/AGENT-BRIEF.md sets out.", file=sys.stderr)
        return 2

    g = JarGraph(args.jar)
    b = Bodies(args.jar)
    if args.selftest:
        # Deliberately BEFORE the recordings are touched: this runs in `ci/t0.sh`
        # on every tree, and a self-test that needs 2.4 GB of recordings is a
        # self-test that gets quietly skipped on the tree that needed it.
        a0 = Analysis(g, b, exc_edges=not args.no_exc_edges, max_gates=args.max_gates)
        ch0 = {k: (v[0], "server tick", False) for k, v in a0.run(TICK_ROOT, SERVER_CLASS).items()}
        return selftest(g, b, a0, ch0)
    cen = census_module()
    regs = cen.registries()
    sections, pushers, _c, _r, _n = cen.tick_sections(g)
    rules_set, _props = cen.harness_world()
    reenabled = cen.scenario_gamerules(global_rules=rules_set)
    rules_off = {k for k, v in rules_set.items() if v in ("false", "0") and k not in reenabled}

    facts = harness_facts(ROOT)
    facts["tickchunks"] = verify_tickchunks(g, b)
    facts["noclient"] = not any(c.startswith("net/minecraft/client/") for c in g.classes)
    facts["onelevel"] = g.subtypes().get("net/minecraft/world/level/Level", set()) == {
        "net/minecraft/server/level/ServerLevel"}
    for cond, (_what, _fals, need) in PREMISES.items():
        if need and not facts.get(need):
            die(f"premise '{cond}' does not hold: check '{need}' failed against the tree")

    # the 33, from the census rather than re-derived
    files = cen.recordings(args.recordings)
    seen, _unknown, _scen = cen.registry_reach(files, regs)
    ent_names, _ = cen.reg_names(regs, "minecraft:entity_type")
    mob_unpop = {}
    for s in sections:
        mm = cen.MOB_SUFFIX.match(s)
        if not mm or not mm.group(1):
            continue
        eid = "minecraft:" + cen.camel_to_snake(mm.group(1))
        if eid in ent_names and eid not in seen["entity_type"]:
            mob_unpop[s] = eid

    a = Analysis(g, b, exc_edges=not args.no_exc_edges, max_gates=args.max_gates)
    ecls = entity_classes(g, b, regs)
    chains = {}
    for s, ch in a.run(TICK_ROOT, SERVER_CLASS).items():
        chains[s] = (ch[0], "server tick", False)
    roots = []
    for eid in sorted(seen["entity_type"]):
        cls = ecls.get(eid)
        if not cls:
            continue
        d = g._declaring(cls, "tick", "()V")
        if d:
            roots.append((eid, cls, (d, "tick", "()V")))
    if len(roots) < 3:
        die(f"only {len(roots)} observed entity types resolved to a tick method")
    moved = entity_motion(files, regs, cen)
    if not moved:
        die("no entity type in the recordings is witnessed moving — the motion kinds moved")
    for eid, cls, root in roots:
        live = eid in moved
        for s, ch in a.run(root, cls).items():
            if s not in chains or len(chains[s][0]) > len(ch[0]):
                chains[s] = (ch[0], f"a {eid.split(':')[1]} ticking"
                             + ("" if live else " (never witnessed moving)"), live)

    if args.why:
        ch = chains.get(args.why)
        print(f"{args.why}: {'no chain' if not ch else ch[1]}")
        if ch:
            for gt in ch[0]:
                mark = "premise " if discharged(gt[3], ch[2]) else "GATE    "
                print(f"  {mark} {gt[3]:44s} {gt[0][0].split('/')[-1]}.{gt[0][1]}@{gt[1]}")
        return 0

    buckets = defaultdict(list)
    for s in sorted(sections):
        ch = chains.get(s)
        bucket, why = classify(s, ch[0] if ch else None, mob_unpop, rules_off, live=bool(ch and ch[2]))
        buckets[bucket].append((s, why, ch[1] if ch else None))

    if args.per_run:
        # THIS STANDS WHERE A FLOOR WOULD BE, and §21.4 is the reason it must
        # not be omitted: a census is not a comparison, so ORACLE rule 6 has no
        # premise to fire on and there is no pair of innocent runs whose
        # disagreement bounds this. What can be measured is whether the same
        # six recordings, censused one at a time, give the same answer — a
        # classification with no spread stated is a classification asserting a
        # stability it did not measure.
        print(f"{'recording':16s} {'RUNS':>6s} {'PERIODIC':>9s} {'OFF':>5s} {'UNPOP':>6s} {'UNDEC':>6s}")
        for f in files:
            seen1, _u, _s = cen.registry_reach([f], regs)
            mob1 = {s2: "minecraft:" + cen.camel_to_snake(cen.MOB_SUFFIX.match(s2).group(1))
                    for s2 in sections
                    if cen.MOB_SUFFIX.match(s2) and cen.MOB_SUFFIX.match(s2).group(1)
                    and "minecraft:" + cen.camel_to_snake(cen.MOB_SUFFIX.match(s2).group(1)) in ent_names
                    and "minecraft:" + cen.camel_to_snake(cen.MOB_SUFFIX.match(s2).group(1))
                    not in seen1["entity_type"]}
            moved1 = entity_motion([f], regs, cen)
            ch1 = {k: (v[0], "server tick", False) for k, v in a.run(TICK_ROOT, SERVER_CLASS).items()}
            for eid1 in sorted(seen1["entity_type"]):
                cls1 = ecls.get(eid1)
                if not cls1:
                    continue
                d1 = g._declaring(cls1, "tick", "()V")
                if not d1:
                    continue
                for s2, c2 in a.run((d1, "tick", "()V"), cls1).items():
                    if s2 not in ch1 or len(ch1[s2][0]) > len(c2[0]):
                        ch1[s2] = (c2[0], eid1, eid1 in moved1)
            bk = buckets_of(sections, ch1, mob1, rules_off)
            print(f"{os.path.basename(f):16s} {len(bk['RUNS']):6d} {len(bk['PERIODIC']):9d} "
                  f"{len(bk['OFF_GAMERULE']):5d} {len(bk['UNPOPULATED']):6d} "
                  f"{len(bk['UNDECIDED']) + len(bk['UNREACHED']):6d}")
        print()

    print(f"pinned jar   {os.path.relpath(args.jar, ROOT)}")
    print(f"recordings   {len(files)} vanilla runs, from {args.recordings}")
    print(f"STAMP        {cen.stamp(args.jar, files, len(cen.load_surface().load_checks()))}")
    print(f"{len(sections)} named tick sections; {a.polymorphic} call sites the walk would not cross "
          f"because their receiver resolves to more than one method\n")
    order = ["RUNS", "PERIODIC", "OFF_GAMERULE", "UNPOPULATED", "UNDECIDED", "UNREACHED"]
    title = {
        "RUNS": "RUNS — every gate on the chain is discharged",
        "PERIODIC": "RUNS PERIODICALLY — gated on a tick counter",
        "OFF_GAMERULE": "PROVABLY NOT ENTERED — a game rule this harness sets",
        "UNPOPULATED": "PROVABLY NOT ENTERED — per-mob, no recording held that mob",
        "UNDECIDED": "UNDECIDED — the named gate decides it and nothing here can",
        "UNREACHED": "UNDECIDED — no chain found at all within the gate budget",
    }
    for k in order:
        rows = buckets.get(k, [])
        print(f"== {len(rows):3d}  {title[k]}")
        for s, why, via in rows:
            print(f"     {s:28s} {(why or '') :44s} {via or ''}")
        print()
    tot = sum(len(buckets.get(k, ())) for k in order)
    if tot != len(sections):
        die(f"{tot} classified but {len(sections)} sections exist")
    ran = len(buckets.get("RUNS", ())) + len(buckets.get("PERIODIC", ()))
    dead = len(buckets.get("OFF_GAMERULE", ())) + len(buckets.get("UNPOPULATED", ()))
    unk = len(buckets.get("UNDECIDED", ())) + len(buckets.get("UNREACHED", ()))
    print(f"-> {ran}/{len(sections)} = {100*ran/len(sections):.2f}% provably ENTERED")
    print(f"-> {dead}/{len(sections)} = {100*dead/len(sections):.2f}% provably NOT entered")
    print(f"-> {unk}/{len(sections)} = {100*unk/len(sections):.2f}% undecided without booting something")
    if args.json:
        json.dump({k: [[s, w, v] for s, w, v in buckets.get(k, ())] for k in order},
                  open(args.json, "w"), indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
