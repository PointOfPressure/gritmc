#!/usr/bin/env python3
"""Every rule the jar declares that a golden table has no row for.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/probe-absences.py            # the absences, from the jar
#     ci/probe-absences.py --check    # refuse if any is unexplained or stale
#     ci/probe-absences.py --table    # markdown, for a cold start
#
# WHAT BREAKS IF YOU IGNORE THIS: a block whose rule the extraction never
# reached is indistinguishable from a block whose rule does nothing, the server
# agrees with the table because both are missing the same input, and no check
# that compares the two can ever fire. That is not hypothetical — see
# `DiodeBlock` below.
# ===== end interface; everything below is why =====

# # A REFUSAL IS LOUD. AN ABSENCE IS NOT.

`ci/refusal-ledger.py` counts what the generators said they could not answer.
This counts what they never said anything about, which is the strictly more
dangerous half, because a refusal has a reason attached and an absence has
nothing at all.

`DiodeBlock.setPlacedBy` is 17 bytes of bytecode:

    if (shouldTurnOn(level, pos, state)) level.scheduleTick(pos, this, 1);

`shouldTurnOn` reads the input side of the world. `UseGolden.TableLevel`
answers **air** for every position it was not told about, so `shouldTurnOn` is
false, no tick is scheduled, the probe found nothing interesting, and
`set_placed_by.json` carried **no row for `minecraft:repeater` or
`minecraft:comparator`**. A consumer reading "no row, no tick" ships a repeater
that never wakes when it is placed next to something already powering it — and
the table and the server hold the same absent input, so they agree, and
agreeing is all any check between them can measure.

**It still answers air, and the repeater has 64 cells.** The fix was not to
throw but to put the read on the row: the cell says `reads: ["air-world"]` and
is emitted *because* it does. "Wrote nothing" and "wrote nothing, in a world
that has nothing to read" are different rows now, and neither is silence.

# # WHICH HARNESSES ARE EXPOSED, AND WHY IT IS A PROPERTY OF ONE METHOD

The exposure is not a property of the table. It is a property of what the
level does with a position it does not know:

  * `ShapeGolden` and `SignalGolden` **throw**. A rule that reaches past what
    the harness holds becomes a refusal with the offset in the message. Their
    absences are measurements — `update_shape.json` establishes inertness by
    sweeping, and `signal.json` publishes `carried + refused + quiet ==
    states` so that "not in the table" has one meaning.
  * `UseGolden.TableLevel` **answers, and records what it answered out of an
    empty world**. It was exposed —
    `blocks.getOrDefault(pos, Blocks.AIR.defaultBlockState())` and a
    `getBlockEntity` returning `null` with a flag nobody read — and that cost
    90 blocks in `use_without_item.json` and 10 in `set_placed_by.json`, every
    one recorded as a decline. It now builds the real `BlockEntity`, refuses
    when vanilla raises its own alarm, and puts `reads: ["air-world"]` and its
    two siblings on any cell whose answer turned on an emptiness — which is
    also what makes the cell survive `interesting()` instead of being dropped.

**Why a third category rather than the repair.** Making `getBlockState` throw,
the `ShapeGolden` discipline, was measured: it moves 1,416 cells of
`use_without_item.json` from a row to a refusal across four classes and reveals
no block that was not already in the table, while in `set_placed_by.json` the
same read is the only thing separating the repeater, the comparator and the
mossy carpet from a block with no rule. One table wants a throw and the other
wants an answer.

All three splits are asserted below against the generators' own source, so a
throwing harness that starts answering, an answering harness that starts
throwing, and a flagging harness that stops flagging each fail here rather than
quietly changing what an absence means.

# # WHAT MAKES IT STAY HONEST

The trigger is the **jar**, never the table: the override set comes from
`javap` over the pinned server jar, so a class the extraction never reached
still appears. Every override with no row must be in
`ci/checks/probe-absences.toml` with a verdict and the read that causes it,
and `--check` refuses three ways:

  * an override in the jar that is neither represented nor explained;
  * an explanation for a class that **is** represented — a stale entry, which
    is how a ledger stops describing the tree;
  * an explanation for a class the jar no longer declares.

The stale arm is not decoration: on 2026-08-18 it retired **18 of the ledger's
22 entries in one run**, naming each, because the environment had closed them
and nothing else in the tree would have said so.

The verdicts are deliberately few, because the interesting one is the first:

    environment   the rule took a branch it would not take in a real world.
                  The table is WRONG here, not merely silent.
    out-of-scope  the rule does something this table does not record at all —
                  a sound, an entity, a screen. Absence is correct for this
                  table and the debt belongs to another one.
    no-op         the override cannot write or schedule under any world.

`environment` is a count of hidden debt and it is meant to be quoted as one.
It was 100 blocks on the morning of 2026-08-18 and it is 3.
"""
import json
import os
import pathlib
import re
import subprocess
import sys
import tomllib
import zipfile

ROOT = pathlib.Path(os.environ.get("PROBE_ABSENCES_ROOT")
                    or pathlib.Path(__file__).resolve().parent.parent)
DATA = ROOT / "data" / "vanilla"
LEDGER = ROOT / "ci" / "checks" / "probe-absences.toml"
GOLDEN = ROOT / "tools" / "goldengen" / "vanilla"
PKG = "net/minecraft/world/level/block/"

# method -> the table it feeds. Only the two exposed ones are cross-referenced;
# see the header for why the other two cannot have this defect.
PROBES = {
    "setPlacedBy": "set_placed_by.json",
    "useWithoutItem": "use_without_item.json",
}

# The structural claim: `(file, the method that answers a position, how many
# `return`s its body may have, whether it must throw)`.
#
# **Counting the returns rather than grepping for the throw**, and the arm in
# `ci/probe-absences-falsify.py` is why. A substring check for the `throw` is
# defeated by *inserting* an early `return` above it: the marker is still
# there, the check stays green, and the harness has silently joined the
# exposed set. So the assertion is over the whole body — a level that answers
# a position it was not given has one more way out than the two it is allowed.
IMMUNE = {
    # subject, then neighbour, then throw.
    "ShapeGolden.java": ("private BlockState stateAt(BlockPos pos)", 2),
    # subject, then throw.
    "SignalGolden.java": ("private BlockState stateAt(BlockPos pos)", 1),
    # subject, neighbour, the one further position the rule was measured to
    # read, then throw. Three ways out and a fourth position still refused —
    # which is the entire claim `update_shape3.json` makes over the census,
    # and the only thing that makes it a table a port could read. One more
    # `return` here and it has started inventing.
    "ShapeThreeGolden.java": ("private BlockState stateAt(BlockPos pos)", 3),
}
# `UseGolden` is the third kind and did not exist until 2026-08-18: a level that
# **answers a default and records that it did**.
#
# It was in `EXPOSED` — `blocks.getOrDefault(pos, AIR)`, one way out, no throw —
# and that was the whole of the defect this file was written for. Moving it to
# `IMMUNE` was measured and rejected: making `getBlockState` throw moves 1,416
# cells of `use_without_item.json` from a row to a refusal across four classes
# and reveals **no block that was not already in the table**, while in
# `set_placed_by.json` the same read is the only thing separating the repeater,
# the comparator and the mossy carpet from a block with no rule. One table wants
# a throw and the other wants an answer.
#
# So the level answers and the *row* carries the fact, and this asserts the two
# halves that make that honest. Either half alone is worthless: a flag nothing
# emits is the `blockEntityAsked` field that sat unread for four months, and an
# emit with no flag is a row that cannot say why it is empty.
#
#   `(file, signature, exact number of ways out, substrings the body must hold)`
#
# **The count is what makes the substrings un-defeatable**, exactly as in
# `IMMUNE` and for the same reason: a change that leaves `readUnheldPosition =
# true` in place and inserts `if (true) return AIR;` above it passes every grep
# and has silently rejoined the exposed set. It cannot pass the count.
FLAGGING = {
    "UseGolden.java": [
        # unheld -> flag, then answer; held -> answer.
        ("public BlockState getBlockState(BlockPos pos)", 2,
         ["readUnheldPosition = true"]),
        # cached; unheld -> flag and null; no block entity declared -> null;
        # declared and unbuildable -> REFUSE; otherwise the real one.
        ("public net.minecraft.world.level.block.entity.BlockEntity "
         "getBlockEntity(BlockPos pos)", 5,
         ["askedBlockEntityElsewhere = true", "return no("]),
        # the emit. A flagged cell that `interesting()` drops is a silent
        # absence again, with a flag set on the way past.
        ("boolean interesting()", 1, ["!reads.isEmpty()"]),
    ],
}
# The one harness that is both, and it has to be both.
#
# `ShapeReadsGolden` runs the same level twice over: with `filler == null` it
# is `ShapeGolden`'s narrow probe and throws, and with a filler it answers
# every position and **invents the world**. That is deliberate — it is how the
# census measures which refusals are artefacts — and it is exactly why nothing
# under `crates/` may read what it produces.
#
# Neither table above can hold it: it has a throw *and* an extra way out, so
# IMMUNE would call it broken and EXPOSED would call it broken the other way.
# Pinning it here says which shape it is supposed to be, so losing either half
# is loud. Losing the throw silently turns the narrow probe into an inventing
# one and every count the census asserts against `update_shape.json` starts
# describing a different extract.
DUAL = {
    "ShapeReadsGolden.java": ("private BlockState stateAt(BlockPos pos)", 3),
}
EXPOSED = {}


def body(src, signature):
    """The braced body that follows `signature`, by brace matching."""
    at = src.index(signature)
    open_at = src.index("{", at)
    depth = 0
    for i in range(open_at, len(src)):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return src[open_at:i + 1]
    raise ValueError(f"unbalanced braces after {signature!r}")


def jar():
    ver = (ROOT / "crates" / "rustmc-proto" / "src" / "lib.rs").read_text()
    ver = ver.split('GAME_VERSION: &str = "')[1].split('"')[0]
    return ROOT / "oracle-work" / f"vanilla-{ver}" / "versions" / ver \
        / f"server-{ver}-unsigned.jar"


def overrides(path):
    """`{method: {simple class name}}`, from `javap` over the pinned jar.

    The whole `world/level/block` package **recursively**, which is the point:
    `PlacedGolden`'s own header derives its family from
    `net/minecraft/world/level/block/*.class` and so counts the base `Block`
    as an override and misses `piston.PistonBaseBlock` — two errors that
    cancel in the total and leave the *set* wrong. The one block that census
    could not see is the only block in that table that refuses.
    """
    with zipfile.ZipFile(path) as z:
        names = [n[:-6].replace("/", ".") for n in z.namelist()
                 if n.startswith(PKG) and n.endswith(".class")]
    out = subprocess.run(["javap", "-p", "-cp", str(path), *sorted(names)],
                         capture_output=True, text=True, check=True).stdout
    found = {m: set() for m in PROBES}
    cur = None
    for line in out.splitlines():
        if "class " in line and line[:1] not in (" ", "\t") and "(" not in line:
            cur = line.split("class ", 1)[1]
            cur = cur.split("<")[0].split(" extends")[0].split(" implements")[0]
            cur = cur.strip().rstrip("{").strip().split(".")[-1]
        else:
            for m in found:
                if f" {m}(" in line:
                    found[m].add(cur)
    # `Block` declares both and overrides neither; it is the base the family
    # is measured against.
    for m in found:
        found[m].discard("Block")
    return found


# The class every table on this level can still be silent about, and it is not
# the block entity.
SERVER_LEVEL = "net/minecraft/server/level/ServerLevel"
PAUSE = "net/minecraft/util/Util.pauseInIde"


def server_level_gates(path, found):
    """`{(class, method): announces}` for every probe override gated on
    `instanceof ServerLevel`.

    # THE SILENCE THE BLOCK ENTITY DID NOT CLOSE

    `SignBlock.useWithoutItem` gets past its block-entity gate and then hits

        if (!(level instanceof ServerLevel serverLevel)) {
            Util.pauseInIde(new IllegalStateException("Expected to only call this on server"));
            return InteractionResult.CONSUME;
        }

    `UseGolden.TableLevel` is not a `ServerLevel` and cannot be —
    `ServerLevel`'s constructor wants a whole server — so that branch is taken
    for every sign in the game, and the recorded row said `consumes: true,
    writes: []`, which is plausible, is right by coincidence, and was reached
    without running a line of the rule.

    `UseGolden` refuses on it now, because vanilla *announces* it:
    `Util.pauseInIde` is handed an exception whose message says what went
    wrong, `SharedConstants.IS_RUNNING_IN_IDE` is `public static boolean` and
    not final, and `Util.setPause` is public. **Exactly one of the eight sites
    below announces it.** The other seven return a plausible value in silence,
    and nothing at runtime can tell them from a rule that genuinely did
    nothing — which is why this is derived from the bytecode here rather than
    detected in the dumper.

    Only the classes that declare a probe method are disassembled — a `javap
    -c` over all 625 classes in the package costs half a minute on every gate
    run, and 70 costs a second.
    """
    want = sorted({c for m in found for c in found[m]})
    if not want:
        return {}
    out = subprocess.run(
        ["javap", "-p", "-c", "-cp", str(path),
         *[PKG.replace("/", ".") + c for c in want]],
        capture_output=True, text=True).stdout
    # `javap` takes fully qualified names and the package is not flat —
    # `piston.PistonBaseBlock` lives a level down — so anything it could not
    # find is retried with the sub-packages the override scan already knows.
    if "Error:" in out:
        out = subprocess.run(
            ["javap", "-p", "-c", "-cp", str(path), *_qualified(path, want)],
            capture_output=True, text=True).stdout
    gates, cls, meth = {}, None, None
    body = []

    def close():
        if cls and meth:
            text = "\n".join(body)
            if f"instanceof" in text and SERVER_LEVEL in text and _gated(text):
                gates[(cls, meth)] = PAUSE.split(".")[-1] in text

    for line in out.splitlines():
        if line and not line[0].isspace() and "class " in line and "(" not in line:
            close()
            cls = line.split("class ", 1)[1].split("<")[0].split(" extends")[0] \
                      .split(" implements")[0].strip().rstrip("{").strip().split(".")[-1]
            meth, body = None, []
        elif line.startswith("  ") and not line.startswith("    ") and "(" in line:
            close()
            meth = next((m for m in PROBES if f" {m}(" in line), None)
            body = []
        elif meth and line.startswith("    "):
            body.append(line)
    close()
    return gates


def _gated(text):
    """Whether an `instanceof` in this body actually names `ServerLevel`.

    A body can mention `ServerLevel` in a descriptor without branching on it —
    `PiglinAi.angerNearbyPiglins(ServerLevel, ...)` is a call, not a gate — and
    counting those would inflate the figure with sites the harness reaches
    perfectly well.
    """
    return re.search(r"instanceof\s+#\d+\s+// class " + re.escape(SERVER_LEVEL),
                     text) is not None


def _qualified(path, want):
    """`package.Class` for each simple name, from the jar's own index."""
    with zipfile.ZipFile(path) as z:
        index = {n[:-6].rsplit("/", 1)[-1]: n[:-6].replace("/", ".")
                 for n in z.namelist()
                 if n.startswith(PKG) and n.endswith(".class")}
    return [index[c] for c in want if c in index]


def registry():
    """`(block id -> class, class -> superclass chain)` for the whole registry.

    From `set_placed_by.json`, which carries both for every block in the game
    and not only for the ones it has rows for — that is what the map is there
    for and its own header says so.
    """
    doc = json.loads((DATA / "set_placed_by.json").read_text())
    return doc["classes"], doc["superclasses"]


def represented(table):
    """Block ids the table says anything at all about."""
    doc = json.loads((DATA / table).read_text())
    return {r["block"] for r in doc["table"]}


def derive():
    classes, supers = registry()

    def chain(name):
        return [name] + supers.get(name, [])

    found = overrides(jar())
    rows = {}
    for method, table in PROBES.items():
        have = represented(table)
        for cls in sorted(found[method]):
            blocks = sorted(b for b, cn in classes.items() if cls in chain(cn))
            if not blocks:
                # An abstract override with no registry block under it. Not an
                # absence: there is nothing for the table to carry.
                continue
            covered = [b for b in blocks if b in have]
            if covered:
                continue
            rows[(method, cls)] = blocks
    return found, rows


# Filled by `gates()`. Module-level because `values()` publishes them and
# `main()` prints them, and threading three collections through both for one
# derivation is worse than saying where they came from.
GATES = {}
GATE_BLOCKS = set()
GATE_SILENT_BLOCKS = set()


def gates(found):
    """Derive the `instanceof ServerLevel` gates and the blocks under them."""
    global GATES
    GATES = server_level_gates(jar(), found)
    classes, supers = registry()

    def chain(name):
        return [name] + supers.get(name, [])

    for (cls, _), announced in GATES.items():
        blocks = {b for b, cn in classes.items() if cls in chain(cn)}
        GATE_BLOCKS.update(blocks)
        if not announced:
            GATE_SILENT_BLOCKS.update(blocks)


def load():
    if not LEDGER.is_file():
        return {}
    doc = tomllib.loads(LEDGER.read_text())
    return {(e["probe"], e["class"]): e for e in doc.get("absent", [])}


def structure():
    """The generators' levels still do what the header claims. `(ok, notes)`."""
    notes = []
    ok = True
    for name, (sig, returns) in IMMUNE.items():
        b = body((GOLDEN / name).read_text(), sig)
        saw, throws = b.count("return "), "throw " in b
        if saw != returns or not throws:
            ok = False
            notes.append(f"{name} no longer throws for a position it was not given "
                         f"({saw} returns and {'a' if throws else 'no'} throw, "
                         f"wanted {returns} and a throw) — if it now answers a "
                         f"default it has joined the exposed set and its table's "
                         f"absences have stopped meaning anything")
        else:
            notes.append(f"{name} throws for an unheld position "
                         f"(absence is a measurement)")
    for name, (sig, returns) in DUAL.items():
        b = body((GOLDEN / name).read_text(), sig)
        saw, throws = b.count("return "), "throw " in b
        if saw != returns or not throws:
            ok = False
            notes.append(f"{name} is no longer both "
                         f"({saw} returns and {'a' if throws else 'no'} throw, "
                         f"wanted {returns} and a throw) — it must throw with no "
                         f"filler and answer one with a filler, because the "
                         f"narrow half is what its counts are asserted against "
                         f"and the wide half is what must never reach crates/")
        else:
            notes.append(f"{name} throws with no filler and invents with one "
                         f"(a census, not an extract)")
    for name, (sig, returns) in EXPOSED.items():
        b = body((GOLDEN / name).read_text(), sig)
        saw, throws = b.count("return "), "throw " in b
        if saw != returns or throws:
            ok = False
            notes.append(f"{name} no longer answers a default for an unheld position "
                         f"({saw} returns and {'a' if throws else 'no'} throw, "
                         f"wanted {returns} and no throw) — if that is a repair, "
                         f"retire the entries in {LEDGER.name} rather than leaving them")
        else:
            notes.append(f"{name} answers air for an unheld position "
                         f"(absence is silent)")
    for name, claims in FLAGGING.items():
        src = (GOLDEN / name).read_text()
        for sig, returns, must in claims:
            short = sig.split("(")[0].split()[-1]
            b = body(src, sig)
            saw = b.count("return ")
            lost = [m for m in must if m not in b]
            if saw != returns or lost:
                ok = False
                notes.append(
                    f"{name} {short} no longer records what it answered out of an "
                    f"empty world ({saw} ways out, wanted {returns}"
                    + (f"; missing {', '.join(repr(m) for m in lost)}" if lost else "")
                    + ") — the absence it hides is silent again, and the entries "
                    f"retired from {LEDGER.name} have to come back")
            else:
                notes.append(f"{name} {short} answers and flags "
                             f"(absence is a row that says why)")
    return ok, notes


def values(rows, tagged, found):
    """The figures this check publishes, by the name a citation quotes.

    Same discipline as `ci/refusal-ledger.py --cite`, and here for the same
    reason: this branch put ten new absence figures into five documents, and a
    figure with nothing tying it to its derivation is the exact thing the other
    half of this ledger spent six commits repairing.
    """
    env = [k for k, e in tagged.items()
           if k in rows and e["verdict"] == "environment"]
    v = {
        "absent.rules": len(rows),
        "absent.blocks": sum(len(b) for b in rows.values()),
        "environment.rules": len(env),
        "environment.blocks": sum(len(rows[k]) for k in env),
    }
    v["serverLevel.gates"] = len(GATES)
    v["serverLevel.silent"] = sum(1 for announced in GATES.values() if not announced)
    v["serverLevel.blocks"] = len(GATE_BLOCKS)
    v["serverLevel.silentBlocks"] = len(GATE_SILENT_BLOCKS)
    for probe in PROBES:
        mine = [k for k in rows if k[0] == probe]
        v[f"{probe}.overrides"] = len(found[probe])
        v[f"{probe}.absent"] = len(mine)
        v[f"{probe}.absentBlocks"] = sum(len(rows[k]) for k in mine)
        for k in mine:
            v[f"{probe}.{k[1]}.blocks"] = len(rows[k])
    return v


def cite(rows, tagged, found):
    """Every sentence quoting one of those figures, checked. See the ledger."""
    doc = tomllib.loads(LEDGER.read_text()) if LEDGER.is_file() else {}
    v = values(rows, tagged, found)
    bad = []
    for c in doc.get("cite", []):
        want = v.get(c["value"])
        if want is None:
            bad.append(("CITATION-UNRESOLVED", c["file"], c["value"],
                        f"not a figure this check derives; it publishes "
                        f"{len(v)} of them"))
            continue
        f = ROOT / c["file"]
        if not f.is_file():
            bad.append(("CITATION-GONE", c["file"], c["value"], "the file is gone"))
            continue
        text = f.read_text()
        quote = c["quote"]
        if any(quote.replace("{}", r) in text for r in (f"{want:,}", str(want))):
            continue
        # Same rule as `ci/refusal-ledger.py`: the number is the run of digits
        # and commas after the head, not whatever splitting on the tail gives,
        # because a quote ending in `{}` has no tail to split on.
        head, _, _tail = quote.partition("{}")
        if head and head in text:
            at = text.index(head) + len(head)
            m = re.match(r"[0-9][0-9,]*", text[at:])
            saw = m.group(0) if m else text[at:at + 10].strip()
            bad.append(("CITATION", c["file"], c["value"],
                        f"quotes {saw!r} where the jar says {want:,}"))
        else:
            bad.append(("CITATION-GONE", c["file"], c["value"],
                        f"no sentence matching {quote!r} — reworded or moved"))
    floor = doc.get("meta", {}).get("min_cites", 0)
    if len(doc.get("cite", [])) < floor:
        bad.append(("CITATION-FLOOR", LEDGER.name, "meta.min_cites",
                    f"{len(doc.get('cite', []))} registered, floor is {floor}"))
    return bad


def _print_gates(where=sys.stdout):
    """The `instanceof ServerLevel` gates, printed wherever the caller wants.

    **Reported, not refused.** A rule behind this gate is not an absence — six
    of the eight have rows — it is a row whose answer was decided by what the
    harness *is*. Refusing them all would delete every chest and barrel row in
    the table over a branch this project may never be able to take, so the
    honest move is to count them where the absences are counted and leave the
    verdict to whoever prices the next harness.
    """
    silent = sorted(k for k, a in GATES.items() if not a)
    loud = sorted(k for k, a in GATES.items() if a)
    print(f"\n`instanceof ServerLevel` — {len(GATES)} probe rule(s) over "
          f"{len(GATE_BLOCKS)} registry blocks", file=where)
    for cls, m in loud:
        print(f"      announces  {cls}.{m}  (Util.pauseInIde — UseGolden refuses on it)",
              file=where)
    for cls, m in silent:
        print(f"      silent     {cls}.{m}", file=where)
    print(f"  {len(silent)} of {len(GATES)} take the branch without a word, over "
          f"{len(GATE_SILENT_BLOCKS)} registry blocks", file=where)
    # **This count is over the two probes this file sweeps, and there is a
    # third.** `useItemOn` has seven more gated rules over 56 registry blocks,
    # all seven silent, and `SignBlock` is in both — it announces on
    # `useWithoutItem` and does not on `useItemOn`, so the 48 blocks that are
    # refusals here are silent rows there. Said in the output rather than only
    # in a doc, because this line is where the number gets read off.
    print(f"      (and 7 more rules over 56 blocks on `useItemOn`, all silent "
          f"— ci/server-level-gates.py sweeps all three)", file=where)


def main():
    if not (DATA / "set_placed_by.json").is_file():
        print("probe-absences: no data/vanilla/set_placed_by.json — "
              "run ci/gen-gameplay-tables.sh", file=sys.stderr)
        return 2
    if not jar().is_file():
        print(f"probe-absences: no jar at {jar()} — this check needs one and "
              f"cannot guess", file=sys.stderr)
        return 2

    ok, notes = structure()
    found, rows = derive()
    tagged = load()
    gates(found)

    unexplained = sorted(k for k in rows if k not in tagged)
    stale = sorted(k for k in tagged
                   if k not in rows and k[1] in found.get(k[0], ()))
    vanished = sorted(k for k in tagged if k[1] not in found.get(k[0], ()))

    cites = cite(rows, tagged, found)
    if "--cite" in sys.argv:
        for kind, f, path, detail in cites:
            print(f"{kind}  {f} [{path}] — {detail}", file=sys.stderr)
        if cites:
            print(f"PROBE-ABSENCE-CITATIONS: {len(cites)} do not match the jar",
                  file=sys.stderr)
            return 1
        n = len(tomllib.loads(LEDGER.read_text()).get("cite", [])) \
            if LEDGER.is_file() else 0
        print(f"PROBE-ABSENCE-CITATIONS OK — {n} sentence(s) quote these "
              f"figures and every one matches")
        return 0

    if "--check" not in sys.argv and "--cite" not in sys.argv and "--table" not in sys.argv:
        _print_gates()

    if "--check" in sys.argv:
        bad = not ok
        for kind, f, path, detail in cites:
            print(f"{kind}  {f} [{path}] — {detail}", file=sys.stderr)
            bad = True
        for note in notes:
            if "no longer" in note:
                print(f"STRUCTURE  {note}", file=sys.stderr)
        for m, c in unexplained:
            print(f"UNEXPLAINED  {m} {c} — {len(rows[(m, c)])} block(s) with no row "
                  f"and no entry in {LEDGER.name}", file=sys.stderr)
            bad = True
        for m, c in stale:
            print(f"STALE  {m} {c} — the table now carries a row for it; "
                  f"drop the entry", file=sys.stderr)
            bad = True
        for m, c in vanished:
            print(f"VANISHED  {m} {c} — the jar no longer declares it", file=sys.stderr)
            bad = True
        if bad:
            print("PROBE-ABSENCES DRIFTED — re-derive "
                  f"{LEDGER.relative_to(ROOT)}", file=sys.stderr)
            return 1
        env = sum(1 for k, e in tagged.items()
                  if k in rows and e["verdict"] == "environment")
        _print_gates(sys.stderr)
        hidden = sum(len(rows[k]) for k, e in tagged.items()
                     if k in rows and e["verdict"] == "environment")
        print(f"PROBE-ABSENCES OK — {len(rows)} absent rules, all explained; "
              f"{env} are the probe's environment, hiding {hidden} blocks")
        return 0

    if "--table" in sys.argv:
        print("| probe | class | blocks | verdict | what it reads |")
        print("|---|---|---:|---|---|")
        for k in sorted(rows):
            e = tagged.get(k, {})
            print(f"| `{k[0]}` | `{k[1]}` | {len(rows[k])} | "
                  f"{e.get('verdict', '**unexplained**')} | {e.get('reads', '—')} |")
        return 0

    for note in notes:
        print(f"  {note}")
    print()
    for method, table in PROBES.items():
        mine = sorted(k for k in rows if k[0] == method)
        blocks = sum(len(rows[k]) for k in mine)
        print(f"{method} -> {table}")
        print(f"  {len(found[method])} overrides in the jar, {len(mine)} with no row, "
              f"covering {blocks} registry blocks")
        for k in mine:
            e = tagged.get(k)
            v = e["verdict"] if e else "UNEXPLAINED"
            reads = f"   reads {e['reads']}" if e else ""
            print(f"    {len(rows[k]):>3}  {k[1]:<26} {v}{reads}")
    if unexplained or stale or vanished:
        print(f"\n{len(unexplained)} unexplained, {len(stale)} stale, "
              f"{len(vanished)} vanished — ci/probe-absences.py --check would refuse")
    return 0


if __name__ == "__main__":
    sys.exit(main())
