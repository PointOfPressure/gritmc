#!/usr/bin/env python3
"""jarcallgraph.py — a method-level call graph of the pinned vanilla jar, in pure Python.

Not a tool on its own. It is the substrate a *census* needs, and it is separate
so that the census reads as a measurement rather than as a class-file parser
with a conclusion at the bottom.

**The consumer this header named until 2026-08-19 was `ci/vanilla-tick-census.py`
and that file has never existed in any ref.** This module sat on `main` gating
nothing, with a pointer to a tool nobody wrote. The consumer is
`ci/mob-tick-census.py`, which uses `subtypes`, `declares` and `reach` to size
the mob surface against ours; the wider tick census the old name implied is
still unwritten. Anyone writing it should read the over-approximation section
below first — it is the reason a "possibly live" answer from this graph is
worth much less than a "dark" one.

# Why not `javap`

`javap -p -c -constants` is the clean-room instrument this project reads
vanilla with, and every *conclusion* here is still checkable that way. But the
census needs every method of all 7,445 classes at once, and javap's text form
costs a JVM per batch and hundreds of megabytes of output to answer a question
about which `invoke` opcodes exist. This reads the same bytes the JVM does,
from the same pinned jar, and nothing else. No decompiler, no other server
implementation, no mappings file.

# What it over-approximates, deliberately

A call graph over bytecode has to decide what `invokevirtual` means. The
opcode names the *static* receiver type; the method that runs is whichever
override the runtime type has. This resolves such a call to **every** subtype
of the named owner that declares the same name and descriptor.

That is an over-approximation, and the direction matters: the census uses this
graph to say "no packet this system can produce was ever observed". An
over-approximated edge set can only *add* packets to a system's reachable set,
so it can only ever make that claim harder to make. A system this graph calls
dark is dark under any sound resolution of the same calls. The complement —
"possibly live" — is correspondingly weak, and the census says so rather than
reporting it as coverage.

Three further approximations, each stated because each bounds a conclusion:

* **Devirtualisation stops at the jar's own package.** A call whose owner is
  outside `net/minecraft/` gets no outgoing edges. The JDK and the third-party
  libraries are not searched for overrides, so a `java.util.function.Consumer`
  call site does not fan out to every lambda in the jar. Lambdas are still
  reached — by the edge below, from the method that *creates* them, which is
  where the interesting attribution lives anyway.
* **`invokedynamic` is resolved through `BootstrapMethods`.** Every
  `MethodHandle` in a bootstrap call site's static arguments becomes a direct
  edge from the enclosing method to the handle's target. That is what makes
  `EntityTickList.forEach(this::tickNonPassenger)` an edge from
  `ServerLevel.tick` to `ServerLevel.tickNonPassenger` rather than a dead end.
* **Reflection and service loading are invisible.** Nothing in the tick path
  found so far uses either, but a system reached only that way would be
  reported dark on no evidence. `--explain` in the census repeats this.
"""

import struct
import zipfile
from collections import defaultdict

# --------------------------------------------------------------------------
# instruction lengths
# --------------------------------------------------------------------------
# Built rather than typed out, so a typo in one row cannot silently shift the
# decode of everything after it. -1 marks the three variable-length opcodes.
ILEN = [1] * 256
ILEN[0x10] = 2  # bipush
ILEN[0x11] = 3  # sipush
ILEN[0x12] = 2  # ldc
ILEN[0x13] = 3  # ldc_w
ILEN[0x14] = 3  # ldc2_w
for _o in range(0x15, 0x1A):  # iload..aload
    ILEN[_o] = 2
for _o in range(0x36, 0x3B):  # istore..astore
    ILEN[_o] = 2
ILEN[0x84] = 3  # iinc
for _o in range(0x99, 0xA9):  # ifeq..jsr
    ILEN[_o] = 3
ILEN[0xA9] = 2  # ret
ILEN[0xAA] = -1  # tableswitch
ILEN[0xAB] = -1  # lookupswitch
for _o in range(0xB2, 0xB9):  # getstatic..invokestatic
    ILEN[_o] = 3
ILEN[0xB9] = 5  # invokeinterface
ILEN[0xBA] = 5  # invokedynamic
ILEN[0xBB] = 3  # new
ILEN[0xBC] = 2  # newarray
ILEN[0xBD] = 3  # anewarray
ILEN[0xC0] = 3  # checkcast
ILEN[0xC1] = 3  # instanceof
ILEN[0xC4] = -1  # wide
ILEN[0xC5] = 4  # multianewarray
ILEN[0xC6] = 3  # ifnull
ILEN[0xC7] = 3  # ifnonnull
ILEN[0xC8] = 5  # goto_w
ILEN[0xC9] = 5  # jsr_w

INVOKES = {0xB6: "virtual", 0xB7: "special", 0xB8: "static", 0xB9: "interface", 0xBA: "dynamic"}


class ClassParseError(Exception):
    pass


def _cp(buf, pos):
    """Constant pool. Returns (entries, pos_after) with entries[0] unused."""
    (count,) = struct.unpack_from(">H", buf, pos)
    pos += 2
    ent = [None] * count
    i = 1
    while i < count:
        tag = buf[pos]
        pos += 1
        if tag == 1:  # Utf8
            (n,) = struct.unpack_from(">H", buf, pos)
            pos += 2
            ent[i] = ("Utf8", buf[pos : pos + n].decode("utf-8", "replace"))
            pos += n
        elif tag in (3, 4):  # Integer, Float
            ent[i] = ("num", struct.unpack_from(">I", buf, pos)[0])
            pos += 4
        elif tag in (5, 6):  # Long, Double — two slots
            ent[i] = ("num64", struct.unpack_from(">Q", buf, pos)[0])
            pos += 8
            i += 1
        elif tag in (7, 8, 16, 19, 20):  # Class, String, MethodType, Module, Package
            ent[i] = ({7: "Class", 8: "String", 16: "MethodType", 19: "Module", 20: "Package"}[tag],
                      struct.unpack_from(">H", buf, pos)[0])
            pos += 2
        elif tag in (9, 10, 11, 12, 17, 18):  # Fieldref/Methodref/IMethodref/NaT/Dynamic/InvokeDynamic
            a, b = struct.unpack_from(">HH", buf, pos)
            ent[i] = ({9: "Fieldref", 10: "Methodref", 11: "IMethodref", 12: "NaT",
                       17: "Dynamic", 18: "InvokeDynamic"}[tag], a, b)
            pos += 4
        elif tag == 15:  # MethodHandle
            k, ref = struct.unpack_from(">BH", buf, pos)
            ent[i] = ("MethodHandle", k, ref)
            pos += 3
        else:
            raise ClassParseError(f"unknown constant pool tag {tag}")
        i += 1
    return ent, pos


class JarGraph:
    """Every `net/minecraft` method in the jar, and what it calls.

    Public state, all keyed by `(internal_class_name, method_name, descriptor)`:

    * `calls[m]`   -> set of unresolved call targets `(owner, name, desc)`
    * `news[m]`    -> set of internal class names constructed with `new`
    * `events[m]`  -> ordered list of `(offset, kind, payload)` where `kind` is
                      `"push"`, `"popPush"`, `"pop"`, `"call"` or `"new"`.
                      This is what lets a caller attribute a call to the
                      profiler section it sits inside.
    * `supers[c]`  -> tuple of the direct supertypes of class `c`
    * `declares[c]`-> set of `(name, desc)` declared by `c`
    """

    PKG = "net/minecraft/"
    PROFILER = "net/minecraft/util/profiling/ProfilerFiller"

    def __init__(self, jar_path, pkg=PKG):
        self.pkg = pkg
        self.calls = defaultdict(set)
        self.fields = defaultdict(set)
        self.news = defaultdict(set)
        self.events = {}
        self.supers = {}
        self.declares = defaultdict(set)
        self.classes = set()
        self._read(jar_path)
        self._subtypes = None

    # ----------------------------------------------------------------- read
    def _read(self, jar_path):
        with zipfile.ZipFile(jar_path) as z:
            for info in z.infolist():
                n = info.filename
                if not n.endswith(".class") or not n.startswith(self.pkg):
                    continue
                self._class(z.read(n))

    def _class(self, buf):
        if buf[:4] != b"\xca\xfe\xba\xbe":
            raise ClassParseError("not a class file")
        pos = 8
        ent, pos = _cp(buf, pos)

        def utf(i):
            e = ent[i]
            return e[1] if e and e[0] == "Utf8" else ""

        def cls(i):
            e = ent[i]
            return utf(e[1]) if e and e[0] == "Class" else ""

        def ref(i):
            """Fieldref/Methodref/IMethodref -> (owner, name, desc)."""
            e = ent[i]
            if not e or e[0] not in ("Fieldref", "Methodref", "IMethodref"):
                return None
            nat = ent[e[2]]
            return (cls(e[1]), utf(nat[1]), utf(nat[2]))

        _flags, this_i, super_i = struct.unpack_from(">HHH", buf, pos)
        pos += 6
        me = cls(this_i)
        self.classes.add(me)
        sup = [cls(super_i)] if super_i else []
        (nif,) = struct.unpack_from(">H", buf, pos)
        pos += 2
        for k in range(nif):
            sup.append(cls(struct.unpack_from(">H", buf, pos + 2 * k)[0]))
        pos += 2 * nif
        self.supers[me] = tuple(s for s in sup if s)

        pos = self._skip_members(buf, pos)  # fields
        # methods
        (nm,) = struct.unpack_from(">H", buf, pos)
        pos += 2
        methods = []
        for _ in range(nm):
            _acc, ni, di = struct.unpack_from(">HHH", buf, pos)
            pos += 6
            name, desc = utf(ni), utf(di)
            self.declares[me].add((name, desc))
            code, pos = self._attrs_find_code(buf, pos, utf)
            methods.append((name, desc, code))
        # class attributes — BootstrapMethods
        bsm, pos = self._attrs_find_bsm(buf, pos, utf)

        for name, desc, code in methods:
            if code is None:
                continue
            self._code(me, name, desc, code, ent, ref, cls, utf, bsm)

    def _skip_members(self, buf, pos):
        (n,) = struct.unpack_from(">H", buf, pos)
        pos += 2
        for _ in range(n):
            pos += 6
            (na,) = struct.unpack_from(">H", buf, pos)
            pos += 2
            for _ in range(na):
                (ln,) = struct.unpack_from(">I", buf, pos + 2)
                pos += 6 + ln
        return pos

    def _attrs_find_code(self, buf, pos, utf):
        (na,) = struct.unpack_from(">H", buf, pos)
        pos += 2
        code = None
        for _ in range(na):
            ni, ln = struct.unpack_from(">HI", buf, pos)
            pos += 6
            if utf(ni) == "Code":
                (clen,) = struct.unpack_from(">I", buf, pos + 4)
                code = buf[pos + 8 : pos + 8 + clen]
            pos += ln
        return code, pos

    def _attrs_find_bsm(self, buf, pos, utf):
        (na,) = struct.unpack_from(">H", buf, pos)
        pos += 2
        bsm = []
        for _ in range(na):
            ni, ln = struct.unpack_from(">HI", buf, pos)
            pos += 6
            if utf(ni) == "BootstrapMethods":
                p = pos
                (n,) = struct.unpack_from(">H", buf, p)
                p += 2
                for _ in range(n):
                    mref, nargs = struct.unpack_from(">HH", buf, p)
                    p += 4
                    args = list(struct.unpack_from(f">{nargs}H", buf, p)) if nargs else []
                    p += 2 * nargs
                    bsm.append((mref, args))
            pos += ln
        return bsm, pos

    # ----------------------------------------------------------------- code
    def _code(self, me, name, desc, code, ent, ref, cls, utf, bsm):
        key = (me, name, desc)
        ev = []
        pending_str = None
        i, n = 0, len(code)
        while i < n:
            op = code[i]
            ln = ILEN[op]
            if op == 0xC4:  # wide
                ln = 6 if code[i + 1] == 0x84 else 4
            elif op in (0xAA, 0xAB):
                p = i + 1
                p += (4 - (p % 4)) % 4
                if op == 0xAA:
                    lo, hi = struct.unpack_from(">ii", code, p + 4)
                    ln = (p + 12 + 4 * (hi - lo + 1)) - i
                else:
                    (np_,) = struct.unpack_from(">i", code, p + 4)
                    ln = (p + 8 + 8 * np_) - i
            if op in (0x12, 0x13):  # ldc / ldc_w
                idx = code[i + 1] if op == 0x12 else struct.unpack_from(">H", code, i + 1)[0]
                e = ent[idx]
                pending_str = utf(e[1]) if e and e[0] == "String" else None
                if pending_str is not None:
                    ev.append((i, "str", pending_str))
            elif 0xB2 <= op <= 0xB5:  # getstatic/putstatic/getfield/putfield
                t = ref(struct.unpack_from(">H", code, i + 1)[0])
                if t:
                    self.fields[key].add(t)
            elif op == 0xBB:  # new
                c = cls(struct.unpack_from(">H", code, i + 1)[0])
                if c:
                    self.news[key].add(c)
                    ev.append((i, "new", c))
            elif op in INVOKES:
                if op == 0xBA:  # invokedynamic
                    e = ent[struct.unpack_from(">H", code, i + 1)[0]]
                    if e and e[0] == "InvokeDynamic" and e[1] < len(bsm):
                        for a in bsm[e[1]][1]:
                            h = ent[a]
                            if h and h[0] == "MethodHandle":
                                t = ref(h[2])
                                if t and t[1]:
                                    self.calls[key].add(t)
                                    ev.append((i, "call", t))
                else:
                    t = ref(struct.unpack_from(">H", code, i + 1)[0])
                    if t:
                        self.calls[key].add(t)
                        ev.append((i, "call", t))
                        if t[0] == self.PROFILER:
                            if t[1] == "push" and t[2] == "(Ljava/lang/String;)V":
                                ev.append((i, "push", pending_str))
                            elif t[1] == "popPush" and t[2] == "(Ljava/lang/String;)V":
                                ev.append((i, "popPush", pending_str))
                            elif t[1] == "pop":
                                ev.append((i, "pop", None))
                    pending_str = None
            i += ln if ln > 0 else 1
        if ev:
            self.events[key] = ev

    # ------------------------------------------------------------ hierarchy
    def subtypes(self):
        """class -> set of all transitive subtypes, itself excluded."""
        if self._subtypes is not None:
            return self._subtypes
        direct = defaultdict(set)
        for c, ss in self.supers.items():
            for s in ss:
                direct[s].add(c)
        out = defaultdict(set)
        for c in self.classes:
            seen, stack = set(), list(direct.get(c, ()))
            while stack:
                d = stack.pop()
                if d in seen:
                    continue
                seen.add(d)
                stack.extend(direct.get(d, ()))
            out[c] = seen
        self._subtypes = out
        return out

    def _declaring(self, owner, name, desc):
        """Nearest ancestor of `owner` that declares `(name, desc)`, or None."""
        seen, stack = set(), [owner]
        while stack:
            c = stack.pop()
            if c in seen:
                continue
            seen.add(c)
            if (name, desc) in self.declares.get(c, ()):
                return c
            stack.extend(self.supers.get(c, ()))
        return None

    def resolve(self, target):
        """One unresolved `(owner, name, desc)` -> the set of methods it may enter.

        Every subtype of the owner that declares the same signature, plus the
        nearest ancestor that does. Owners outside the jar's package resolve to
        nothing: see the module docstring.
        """
        owner, name, desc = target
        if not owner.startswith(self.pkg):
            return frozenset()
        out = set()
        d = self._declaring(owner, name, desc)
        if d:
            out.add((d, name, desc))
        for s in self.subtypes().get(owner, ()):
            if (name, desc) in self.declares.get(s, ()):
                out.add((s, name, desc))
        return frozenset(out)

    def edges(self):
        """The resolved graph: method -> set(method). Memoised."""
        if getattr(self, "_edges", None) is not None:
            return self._edges
        cache = {}
        out = defaultdict(set)
        for m, ts in self.calls.items():
            for t in ts:
                r = cache.get(t)
                if r is None:
                    r = cache[t] = self.resolve(t)
                out[m] |= r
        self._edges = out
        return out

    def reach(self, roots):
        """Transitive closure over `edges()`, roots included."""
        e = self.edges()
        seen, stack = set(), list(roots)
        while stack:
            m = stack.pop()
            if m in seen:
                continue
            seen.add(m)
            stack.extend(e.get(m, ()))
        return seen
