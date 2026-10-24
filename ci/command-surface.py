#!/usr/bin/env python3
"""Generate the registered-command surface: vanilla's, ours, and the corpus's."""

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/command-surface.py --check     refuse if docs/generated/command-surface.tsv
#                                       disagrees with the jar, our source, or the corpus
#     ci/command-surface.py --write     regenerate that file
#     ci/command-surface.py --worklist  print the unported roots, cheapest first
#
# WHAT BREAKS IF YOU IGNORE THIS: you paste a table of commands into a doc, it
# is right on the day and wrong the week after, and an orchestrator turn is
# spent correcting it. Three separate corrections on 2026-08-18 came from
# exactly that. There is no hand-written command table in this repo and there
# must not be one.
# ===== end interface; everything below is why =====

# # WHY THE REPORT ALONE IS THE WRONG ANSWER
#
# `oracle-work/vanilla-26.2/generated/reports/commands.json` is vanilla's own
# dump of the Brigadier tree, so it is tempting to call its 92 root children
# "the command surface" and stop. That is wrong by two commands, and the two
# it is wrong by are the ones a *dedicated* server never has.
#
# `net.minecraft.data.info.CommandsReport` constructs
#
#     new Commands(Commands.CommandSelection.ALL, ctx)
#
# — read from the jar, not assumed; the generator asserts it below. `ALL` sets
# both `includeIntegrated` and `includeDedicated`, and `Commands.<init>` gates
# three blocks on those two fields:
#
#     650: getfield includeDedicated  / 654: ifeq 762     <- 15 classes
#     763: getfield includeIntegrated / 766: ifeq 783     <-  2 classes
#     636: getfield includeDedicated  / 639: ifeq 650     <-  1 class, and
#                                                            also inside the
#                                                            dev-flag guard
#
# so the report is a strict superset of a dedicated server's tree. Subtracting
# the integrated-only block is the whole difference between a list that
# describes the server we are differing against and a list that does not.
#
# # WHY THE GUARDS ARE READ STRUCTURALLY AND NOT BY OFFSET
#
# The offsets above are this jar's. Hard-coding them makes the script a lie
# the moment the jar moves. Instead the guard intervals are recovered from the
# instruction stream: a `getfield` of one of the two selection fields followed
# by `ifeq T` means "if the flag is clear, jump past the block", so the block
# is exactly the half-open offset interval [after-the-branch, T). Every
# `register` call whose offset falls in that interval belongs to that
# selection. Nothing else about the constructor is assumed.
#
# # THE ROOT NAME COMES FROM BYTECODE, AND THE SCRIPT REFUSES WHEN IT CANNOT
#
# A class name is not a command name — `TeleportCommand` registers `teleport`
# *and* `tp`, and `BanIpCommands` registers `ban-ip`, which no mechanical
# transform of the class name produces. So the root literal is read from the
# class's own `register` method: the first string handed to `Commands.literal`
# or `LiteralArgumentBuilder.literal`.
#
# **That rule is only sound for the inline builder shape**
#
#     dispatcher.register(literal("root").then(literal("child")...))
#
# and it is unsound for the shape `StopSoundCommand` uses, where the subtree
# is built into a local first and `literal("stopsound")` is reached *last*.
# Under the first-literal rule `StopSoundCommand` yields `*`, which is a child.
#
# The script therefore does not apply the rule to all 93 registrations. It
# applies it only to the 18 classes inside the three guarded blocks, where the
# answer is needed, and then refuses unless every one of them satisfies the
# preconditions that make it sound:
#
#   * exactly one `CommandDispatcher.register` call in the class, so there is
#     one root to find and not two;
#   * the recovered literal is a root of the report, so a child literal that
#     slipped through cannot pass silently;
#   * no two classes recover the same root.
#
# The unguarded roots are then obtained by *subtraction* from the report
# rather than by running the unsound rule over the remaining 75 classes. The
# fragile step is confined to the 18 places it is known to hold and is checked
# three ways in each of them.
#
# # WHAT THIS FILE STILL CANNOT TELL YOU, AND WHAT CAN
#
# Everything above is static. It says what the jar *would* register, and two
# static derivations that agree can still agree about the wrong thing. The
# non-tautological check is the wire: a real dedicated vanilla server sends
# `ClientboundCommandsPacket` to every player on join, and its root set is the
# answer with no bytecode reading in it at all. `ci/checks/gameplay/113-*`
# records exactly that, and `--check` compares it against this derivation when
# a recording is present. Until one is, `--check` says so rather than
# reporting a pass it did not earn.

import argparse, json, os, re, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VER = re.search(r'GAME_VERSION: &str = "([^"]+)"',
                (ROOT / "crates" / "rustmc-proto" / "src" / "lib.rs").read_text()).group(1)
ORACLE = ROOT / "oracle-work" / f"vanilla-{VER}"
JAR = ORACLE / "versions" / VER / f"server-{VER}.jar"
REPORT = ORACLE / "generated" / "reports" / "commands.json"
OUT = ROOT / "docs" / "generated" / "command-surface.tsv"
SCEN = ROOT / "ci" / "checks" / "gameplay"
OURS_SRC = ROOT / "crates" / "rustmc-server" / "src" / "commands.rs"
DUMP = ROOT / "data" / "vanilla" / "command_surface.json"
REGISTRIES = ORACLE / "generated" / "reports" / "registries.json"

SEL = "net/minecraft/commands/Commands$CommandSelection"
DISPATCH_REG = "com/mojang/brigadier/CommandDispatcher.register:"
LITERALS = ("net/minecraft/commands/Commands.literal:",
            "com/mojang/brigadier/builder/LiteralArgumentBuilder.literal:")


class Refusal(Exception):
    pass


_JAVAP_CACHE = {}


def javap(cls):
    """Disassembly of one class, from a batched call.

    Batched because the check runs inside `ci/t2-gameplay.sh`, which holds the
    corpus lock that has been six deep in a day. One `javap` per class is 28
    JVM starts and 71 seconds of a contended gate, nearly all of it process
    startup; asking for all 28 at once is three. A gate expensive enough that
    people route around it is a gate that is not run.
    """
    if cls not in _JAVAP_CACHE:
        javap_many([cls])
    if cls not in _JAVAP_CACHE:
        raise Refusal(f"javap produced nothing for {cls}")
    return _JAVAP_CACHE[cls]


def javap_many(classes):
    """Disassemble many classes in one JVM start, filling the cache."""
    want = [c.replace("/", ".") for c in classes if c.replace("/", ".") not in _JAVAP_CACHE]
    if not want:
        return
    r = subprocess.run(["javap", "-c", "-p", "-classpath", str(JAR), *want],
                       capture_output=True, text=True)
    if r.returncode != 0 and not r.stdout.strip():
        raise Refusal(f"javap failed: {r.stderr.strip()[:300]}")
    # Each class's output starts at its `Compiled from` banner. The class name
    # is on the declaration line that follows, which is the only place javap
    # states it, so the chunks are keyed off that rather than off the order of
    # the arguments -- javap is free to reorder and to omit.
    for chunk in r.stdout.split("Compiled from ")[1:]:
        m = re.search(r"^(?:[\w$]+\s+)*(?:class|interface|enum|record)\s+([\w.$]+)",
                      chunk, re.M)
        if m:
            _JAVAP_CACHE[m.group(1)] = chunk
    missing = [c for c in want if c not in _JAVAP_CACHE]
    if missing:
        raise Refusal(f"javap produced nothing for {missing}")


def method_body(text, sig_re):
    m = re.search(sig_re + r".*?\n(.*?)(?=\n  \S|\Z)", text, re.S)
    if not m:
        raise Refusal(f"no method matching {sig_re!r}")
    return m.group(1)


def instructions(body):
    out = []
    for ln in body.split("\n"):
        m = re.match(r"\s*(\d+):\s+(\S+)\s*(.*)", ln)
        if m:
            out.append((int(m.group(1)), m.group(2), m.group(3)))
    return out


def assert_report_is_all():
    """The report is only a superset if CommandsReport asked for ALL."""
    t = javap("net.minecraft.data.info.CommandsReport")
    if f"{SEL}.ALL" not in t:
        raise Refusal("CommandsReport does not reference CommandSelection.ALL; "
                      "the report's relationship to a dedicated server is unknown")


CONDJUMP = ("ifeq", "ifne", "iflt", "ifge", "ifgt", "ifle",
            "if_icmpeq", "if_icmpne", "if_acmpeq", "if_acmpne",
            "ifnull", "ifnonnull")


def guard_intervals(ins):
    """[(label, lo, hi)] for every forward conditional branch in the method.

    A forward conditional branch at offset `o` with target `T` means the
    instructions in [after-the-branch, T) run only under some condition, so
    that half-open interval is a guarded block. The label is the nearest
    preceding `getfield`/`getstatic`/`invoke*` symbol — the thing whose value
    was tested. Two selection labels are recognised by name and everything
    else is reported verbatim, so a guard this script has never seen shows up
    as a guard rather than as nothing.

    An OR-chain (`DEBUG_DEV_COMMANDS ifne L; IS_RUNNING_IN_IDE ifeq END`)
    yields the right *interval* but is labelled with only its last term. That
    is why the label is never load-bearing here: the classification that
    matters is "conditional at all", and only the two selection guards are
    ever read by name.
    """
    out, src = [], None
    for i, (off, op, rest) in enumerate(ins):
        m = re.search(r"// (?:Field|Method|InterfaceMethod) (\S+)", rest)
        if m:
            src = m.group(1).split(":")[0]
        if op not in CONDJUMP:
            continue
        target = int(rest.split()[0])
        if target <= off:
            continue
        label = "DEDICATED" if src and src.endswith("includeDedicated") else \
                "INTEGRATED" if src and src.endswith("includeIntegrated") else \
                f"cond:{src or '?'}"
        out.append((label, ins[i + 1][0] if i + 1 < len(ins) else off, target))
    return out


def registrations():
    """{class -> set(labels)} for every *Command.register call in Commands.<init>."""
    t = javap("net.minecraft.commands.Commands")
    body = method_body(t, r"\n  public net\.minecraft\.commands\.Commands\(")
    ins = instructions(body)
    guards = guard_intervals(ins)
    for want in ("DEDICATED", "INTEGRATED"):
        if not any(g[0] == want for g in guards):
            raise Refusal(f"no {want} selection guard in Commands.<init>; the "
                          "constructor's shape has changed")
    calls = {}
    for off, op, rest in ins:
        m = re.search(r"// Method (net/minecraft/server/commands/[\w$]+)\.register:", rest)
        if not m:
            continue
        cls = m.group(1)
        if cls in calls:
            raise Refusal(f"{cls}.register is called twice in Commands.<init>; "
                          "its selection is ambiguous")
        calls[cls] = {lab for lab, lo, hi in guards if lo <= off < hi}
    return calls


def root_of(cls):
    """First literal in `cls.register`. Sound only under the checks in caller."""
    t = javap(cls.replace("/", "."))
    if t.count(DISPATCH_REG) != 1:
        raise Refusal(f"{cls} has {t.count(DISPATCH_REG)} dispatcher.register calls; "
                      "the first-literal rule needs exactly one")
    body = method_body(t, r"\n  [\w. ]*register\(")
    last = None
    for ln in body.split("\n"):
        m = re.search(r"ldc(?:_w)?\s+#\d+\s+// String (.*)$", ln.strip())
        if m:
            last = m.group(1)
            continue
        if any(l in ln for l in LITERALS) and last is not None:
            return last
    raise Refusal(f"{cls}.register has no literal string")


def vanilla_surface():
    """(all report roots, {root: (class, guards)}, dedicated-only, integrated-only).

    Roots are recovered from bytecode only for the classes inside a guard,
    because those are the only ones whose selection differs from "everyone
    gets it" and because the first-literal rule is checked, not trusted. The
    unguarded roots need no recovery at all: they are the report minus the
    guarded ones, and subtraction cannot pick up a child literal by mistake.

    A class inside a *non-selection* guard — the JFR probe, the dev-command
    flags — may legitimately be absent from the report, because the report was
    taken with those flags in whatever state the generator ran under. Such a
    root is not required to be in the report, and if it is absent it is not
    part of a normal server's surface either. `debugconfig` is the worked
    example: it sits inside the dev guard *and* the dedicated guard, and it is
    in neither the report nor any server a player connects to.
    """
    assert_report_is_all()
    report = json.loads(REPORT.read_text())
    if report.get("type") != "root":
        raise Refusal("commands.json is not a root node")
    all_roots = set(report["children"])
    calls = registrations()
    guarded = {c: labs for c, labs in calls.items() if labs}
    javap_many(guarded)
    owned, seen, offstage = {}, {}, {}
    for cls, labs in sorted(guarded.items()):
        r = root_of(cls)
        conditional = [l for l in labs if l.startswith("cond:")]
        if r not in all_roots:
            if not conditional:
                raise Refusal(
                    f"{cls} recovered root {r!r}, which is not a root of the "
                    "report and is under no conditional guard; the "
                    "first-literal rule picked up a child")
            offstage[r] = (cls, tuple(sorted(labs)))
            continue
        if r in seen:
            raise Refusal(f"{cls} and {seen[r]} both recovered root {r!r}")
        seen[r] = cls
        owned[r] = (cls, tuple(sorted(labs)))
    ded = {r for r, (_, l) in owned.items() if "DEDICATED" in l}
    integ = {r for r, (_, l) in owned.items() if "INTEGRATED" in l}
    if ded & integ:
        raise Refusal(f"roots in both selections: {sorted(ded & integ)}")
    return all_roots, owned, ded, integ, offstage


def ours():
    """Root literals this server registers, read from `build_tree`.

    Depth matters and a flat regex gets it wrong: the first version of this
    function matched every `Node::leaf("...")` in the function and reported
    `uuids` — a *child* of `list` — as a root this server registers and
    vanilla does not. The generator caught that on its first run, which is the
    argument for generating the list rather than writing it down.

    So the `roots: vec![...]` block is walked with a bracket counter and only
    the leaves at depth one, directly inside the vec, are roots.
    `tests::the_generated_surface_names_our_actual_roots` asserts this against
    the tree the server really builds, so a command added in Rust without
    regenerating fails from the Rust side rather than waiting for a reader to
    notice.
    """
    src = OURS_SRC.read_text()
    i = src.find("roots: vec![")
    if i < 0:
        raise Refusal("no `roots: vec![` in commands.rs")
    i += len("roots: vec![")
    depth, start = 1, i
    while i < len(src) and depth:
        if src[i] in "([{":
            depth += 1
        elif src[i] in ")]}":
            depth -= 1
        i += 1
    block, out, depth = src[start:i - 1], [], 0
    # COMMENTS ARE STRIPPED, AND THEY WERE NOT — 2026-09-05. Every line in this
    # block that names a vanilla registration in prose — "`literal(\"say\")`",
    # "`Node::leaf(\"x\")`" — is input to the scan below, so a doc comment
    # describing what a root corresponds to in the jar could add a root, remove
    # one by unbalancing the bracket counter, or (as it did) trip the
    # builder-with-arguments refusal on the word `literal`. Code only.
    block = re.sub(r"//[^\n]*", "", block)
    # A root is either a `Node::leaf("x")` written inline at depth one, or a
    # call to a builder function — `gamerule_root()` — because a root with 116
    # children is not something to write inline. A builder is resolved by
    # finding `fn <name>(` in this same file and taking the FIRST
    # `Node::leaf("...")` in it, which is the root the builder starts from.
    #
    # Resolving it rather than ignoring it is the point: an unresolvable
    # builder is a refusal, so a root registered through a shape this parser
    # does not understand stops the generator instead of quietly shrinking the
    # list. That is the same argument as the depth counter below, which exists
    # because a flat regex once reported `uuids` — a *child* of `list` — as a
    # root of its own.
    # `\(\)` ONLY, AND THAT WAS A HOLE — 2026-09-05. A builder taking
    # arguments — `message_root("say", "message", Level::Gamemasters)` — matched
    # neither alternative, so its name fell through to the bracket counter,
    # its own parentheses balanced, and the root was **silently dropped**. Two
    # roots vanished from the surface and the generator said nothing; only
    # `tests::the_generated_surface_names_our_actual_roots` caught it, from the
    # Rust side, which is the backstop and not the check.
    #
    # This is the same class the depth counter and the unresolvable-builder
    # refusal below were both written for, arriving through the one shape
    # neither covered. So the pattern now matches a call with arguments too,
    # and such a call is a REFUSAL rather than a drop: a builder whose root
    # literal is a parameter cannot be named by reading this file, and saying
    # so is the only honest answer.
    #
    # The lookbehind on both call alternatives is load-bearing and the first
    # version did not have it: without it `.requires(Level::Gamemasters)` and
    # `.then(...)` match as builder calls, and every chained root becomes a
    # refusal. A builder call is an identifier that follows neither `.` nor
    # `:`.
    #
    # AND A THIRD SHAPE, FOUND THE SAME WAY — 2026-09-05.
    # `Node::redirect_to("tell", &["msg"])` matched neither `Node::leaf(...)`
    # nor either call alternative (the lookbehind excludes an identifier after
    # `::`), so `tell` and `w` were dropped in silence exactly as the
    # with-arguments builders had been. The lesson is not "add redirect_to": it
    # is that any `Node::<something>(` at depth one names a root and this
    # parser must either know it or STOP. The last alternative is that
    # backstop.
    for m in re.finditer(
            r'[()\[\]{}]'
            r'|Node::leaf\("([^"]+)"\)'
            r'|Node::redirect_to\("([^"]+)"'
            r'|(?<![.:\w])([a-z_][a-z0-9_]*)\(\)'
            r'|(?<![.:\w])([a-z_][a-z0-9_]*)\(\s*[^)]'
            r'|Node::([a-z_][a-z0-9_]*)\(',
            block):
        if m.group(2) is not None:
            if depth == 0:
                out.append(m.group(2))
            # `redirect_to(` opened a paren the counter has not seen.
            depth += 1
            continue
        if m.group(5) is not None:
            if depth == 0:
                raise Refusal(
                    f"`Node::{m.group(5)}(...)` is used at the top of "
                    f"`roots: vec![]` and this parser does not know what root it "
                    f"names. Teach it, or the surface silently loses a root.")
            depth += 1
            continue
        if m.group(1) is not None:
            if depth == 0:
                out.append(m.group(1))
            continue
        if m.group(3) is not None:
            if depth == 0:
                fn = m.group(3)
                body = src.find(f"fn {fn}()")
                if body < 0:
                    raise Refusal(f"root builder {fn}() is called in `roots: vec![]` "
                                  f"and `fn {fn}()` is not in {OURS_SRC.name}")
                # THE SEARCH IS BOUNDED TO THE BUILDER'S OWN BODY, AND IT WAS
                # NOT — 2026-09-05. `src[body:]` runs to the end of the file, so
                # a builder whose body holds no literal `Node::leaf("x")` —
                # because it delegates, or because its literal is a parameter —
                # silently adopted the next one further down the file. Two roots
                # both resolved to `gamerule` that way and `sorted(set(...))`
                # deduplicated the evidence away.
                #
                # Same class as the depth counter and the with-arguments
                # refusal: a shape this parser cannot read must stop it, not
                # shrink its answer. The body is brace-matched from the `{`
                # after the signature.
                open_brace = src.find("{", body)
                if open_brace < 0:
                    raise Refusal(f"root builder {fn}() has no body")
                j, d = open_brace + 1, 1
                while j < len(src) and d:
                    if src[j] == "{":
                        d += 1
                    elif src[j] == "}":
                        d -= 1
                    j += 1
                leaf = re.search(r'Node::leaf\("([^"]+)"\)', src[open_brace:j])
                if not leaf:
                    raise Refusal(
                        f"root builder {fn}() has no literal Node::leaf(\"x\") in its "
                        f"own body, so this parser cannot name the root it registers")
                out.append(leaf.group(1))
            # The `()` of the call was consumed by this match, so the depth
            # counter must not see it.
            continue
        if m.group(4) is not None:
            if depth == 0:
                raise Refusal(
                    f"root builder {m.group(4)}(...) is called with arguments in "
                    f"`roots: vec![]`; this parser can only name a root from a "
                    f"literal `Node::leaf(\"x\")` or a zero-argument builder, so it "
                    f"cannot say what root that call registers. Give it a "
                    f"zero-argument wrapper.")
            # The opening paren was consumed; the closing one has not been, so
            # let the counter see the match's own bracket and nothing else.
            depth += 1
            continue
        if m.group(0) in "([{":
            depth += 1
        else:
            depth -= 1
    if not out:
        raise Refusal("no root Node::leaf in commands.rs")
    return sorted(set(out))


def corpus_invocations():
    """{root -> [scenario names]} for every `do = "command"` step.

    Parsed per `[[step]]` block rather than with one regex across the file.
    The first version matched `do = "command"` followed immediately by `text`,
    which is the order the three existing scenarios happen to use and is not
    something TOML promises: a step with a comment between the two keys, or the
    keys the other way round, would have been silently invisible.
    """
    hits = {}
    for p in sorted(SCEN.glob("*.toml")):
        for block in re.split(r"^\s*\[\[step\]\]\s*$", p.read_text(), flags=re.M)[1:]:
            verb = re.search(r'^\s*do\s*=\s*"([^"]*)"', block, re.M)
            text = re.search(r'^\s*text\s*=\s*"([^"]*)"', block, re.M)
            if not verb or verb.group(1) != "command" or not text:
                continue
            words = text.group(1).split()
            hits.setdefault(words[0] if words else "", []).append(p.stem)
    return hits


def executed_surface():
    """The same question asked of a *running* dispatcher, or None.

    `ci/command-surface-dump.sh` constructs a real
    `new Commands(CommandSelection.DEDICATED, ctx)` and reports what came out.
    Everything else in this file is static: bytecode read with javap and a
    report read off disk. Two static readings of one jar that agree have
    agreed about a jar, not about a server.

    This is the derivation that can contradict them, and the contradiction is
    the point. It also carries `usableAtAll` — the roots that survive
    `fillUsableCommands` for a `PermissionLevel.ALL` source, which is what the
    gameplay corpus's players hold and therefore the only part of the surface
    any scenario can reach.
    """
    if not DUMP.exists():
        return None
    d = json.loads(DUMP.read_text())
    if d.get("selection") != "DEDICATED":
        raise Refusal(f"{DUMP.name} was not dumped for CommandSelection.DEDICATED")
    return set(d["registered"]), set(d["usableAtAll"])


def cost_of(node, parsers):
    """(literals, arguments, redirects, depth) for one root's subtree.

    The units are the ones that decide whether a command can be *registered*
    at all, which is the cost that comes first and the one this repo's
    `commandtree.rs` makes a type error rather than a wrong byte. A root whose
    subtree is all literals is expressible by today's `Node`; one argument
    node anywhere in it is not, however small the reply.

    `parsers` accumulates the argument-type ids the subtree needs, because
    "add argument nodes" is not one job: each parser id is its own
    `ArgumentTypeInfo` serialiser, and the ones that write no properties are
    an afternoon while the ones that write a suggestion provider are not.
    """
    lit = arg = red = 0
    depth = 0
    kind = node.get("type")
    if kind == "literal":
        lit = 1
    elif kind == "argument":
        arg = 1
        if "parser" in node:
            parsers.add(node["parser"])
    if "redirect" in node:
        red = 1
    for c in node.get("children", {}).values():
        l, a, r, d = cost_of(c, parsers)
        lit += l
        arg += a
        red += r
        depth = max(depth, d)
    return lit, arg, red, depth + 1


def parser_ids():
    """{registry name -> protocol id} for command argument types, from the jar."""
    d = json.loads(REGISTRIES.read_text())
    key = "minecraft:command_argument_type"
    if key not in d:
        raise Refusal(f"{REGISTRIES.name} has no {key}")
    return {n: e["protocol_id"] for n, e in d[key]["entries"].items()}


def build_rows():
    all_roots, owned, ded, integ, offstage = vanilla_surface()
    dedicated_surface = sorted(all_roots - integ)
    ex = executed_surface()
    usable = None
    if ex is not None:
        registered, usable = ex
        if registered != set(dedicated_surface):
            only_static = sorted(set(dedicated_surface) - registered)
            only_run = sorted(registered - set(dedicated_surface))
            raise Refusal(
                "the static and executed derivations of the dedicated surface "
                f"disagree. Read off the bytecode but not registered: {only_static}. "
                f"Registered but not derived: {only_run}. One of the two is wrong "
                "and neither may be believed until it is known which.")
        if not usable < registered:
            raise Refusal("every registered root is usable at PermissionLevel.ALL; "
                          "the permission filter cannot be being applied")
    report = json.loads(REPORT.read_text())["children"]
    ids = parser_ids()
    by_id = {v: k for k, v in ids.items()}
    o, inv = set(ours()), corpus_invocations()
    rows = []
    for r in dedicated_surface:
        cls, labs = owned.get(r, ("", ()))
        cond = [l for l in labs if l.startswith("cond:")]
        sel = ("dedicated-only" if "DEDICATED" in labs
               else "conditional" if cond else "always")
        reach = "-" if usable is None else ("level0" if r in usable else "op")
        parsers = set()
        lit, arg, red, depth = cost_of(report[r], parsers)
        # The one column that says what to do next. `literals` means today's
        # Node can express the whole subtree; anything else names the barrier
        # rather than a number, because "3 arguments" and "1 argument" are the
        # same job and "an argument and a redirect" is two.
        blocked = []
        if arg:
            blocked.append("arg")
        if red:
            blocked.append("redirect")
        shape = "+".join(blocked) or "literals"
        names = sorted(by_id.get(p, str(p)).removeprefix("minecraft:") for p in parsers)
        rows.append((r, sel, reach, cls.split("/")[-1], "yes" if r in o else "no",
                     shape, str(lit), str(arg), str(red), str(depth),
                     ",".join(names) or "-",
                     ",".join(sorted(set(inv.get(r, [])))) or "-"))
    # An invocation of something that is not a dedicated-server root would
    # otherwise vanish: the table is built by walking the surface and looking
    # each root up in `inv`, so a scenario running `/publish`, `/raid` or a typo
    # contributes to no row and reads as no invocation at all. Surfaced rather
    # than refused, because a scenario written to exercise the unknown-command
    # reply is legitimate future work -- and because the drift check means
    # adding one changes this file and gets read.
    stray = sorted(k for k in inv if k not in set(dedicated_surface))
    return (rows, sorted(integ), sorted(o - set(dedicated_surface)), sorted(offstage),
            usable, stray)


HEADER = ("root\tselection\tcorpus_reach\tvanilla_class\tported\tshape\tliterals\targuments\tredirects\tdepth\tparsers\tinvoked_by")


def render():
    rows, integ, extra, offstage, usable, stray = build_rows()
    n_ported = sum(1 for r in rows if r[4] == "yes")
    n_inv = sum(1 for r in rows if r[11] != "-")
    lvl0 = [r for r in rows if r[2] == "level0"]
    ready = [r[0] for r in lvl0 if r[5] == "literals" and r[4] == "no"]
    # The same filter without the reachability clause. Kept separate and both
    # printed, because reporting only the first over-generalised into "there is
    # no cheap end" when what is true is narrower: nothing *the corpus can see*
    # is expressible by today's Node, while ten roots an operator sees are.
    # Those ten need no argument nodes and no redirects, and
    # ci/commands-golden.sh gates their bytes at op levels with vanilla's own
    # encoder, so they are portable without the type work -- what stops them is
    # each one's reply, not its tree.
    ready_any = [r[0] for r in rows if r[5] == "literals" and r[4] == "no"]
    out = [
        "# GENERATED by ci/command-surface.py --write. Do not edit; the check in",
        "# ci/command-surface.py --check refuses a copy that has drifted.",
        "#",
        "# Roots a *dedicated* vanilla 26.2 server registers, derived twice: from",
        "# the CommandSelection.ALL report minus the includeIntegrated block that",
        "# Commands.<init> gates, and from a real DEDICATED dispatcher built by",
        "# ci/command-surface-dump.sh. The two must agree or this refuses.",
        f"# integrated-only, so absent from a dedicated server: {' '.join(integ)}",
        f"# dev-flag-gated, so absent from both: {' '.join(offstage) or '(none)'}",
        "#",
        "# corpus_reach is the column that decides what is worth porting.",
        "# fillUsableCommands filters the tree per source, and the gameplay",
        "# corpus's players hold PermissionLevel.ALL, so `op` roots are ones no",
        "# scenario can invoke or even see in its declare-commands packet.",
        f"# roots {len(rows)}  level0 {len(lvl0)}"
        f"  ported {n_ported}  invoked-by-corpus {n_inv}",
        "#",
        "# shape names what a root needs from the Node type: `literals` is a",
        "# bare literal subtree, `arg` needs an argument node, `redirect` needs",
        "# a redirect edge. THIS COLUMN NO LONGER DECIDES WHETHER A ROOT CAN BE",
        "# REGISTERED, and it said so until 2026-09-05: commandtree.rs carried",
        "# argument nodes, redirects and custom suggestions from stack-extract",
        "# onwards, and /msg, /tell and /w are `arg` and `redirect` roots this",
        "# server ships. What shape still measures is how much of the ARGUMENT",
        "# work a root needs; parsers names the ArgumentTypeInfo serialisers,",
        "# and the ones Parser::parse transcribes are in the golden at",
        "# ci/command-parse-golden.sh.",
        f"# level0 and shape=literals and not ported: {' '.join(ready) or '(none)'}",
        f"# any level, shape=literals, not ported: {' '.join(ready_any) or '(none)'}",
        "# The second list is portable with today's Node and gated at op levels",
        "# by ci/commands-golden.sh. No scenario can see them, so each buys a",
        "# byte-exact tree and no corpus movement; what each still costs is its",
        "# reply. /version is the worked example and the warning: its tree is one",
        "# literal and its reply embeds Date.toString() in the server's zone.",
    ]
    if usable is None:
        out.append("# NO EXECUTED DUMP PRESENT — corpus_reach is unknown and the two")
        out.append("# derivations have not been compared. Run ci/command-surface-dump.sh.")
    if extra:
        out.append(f"# WE REGISTER ROOTS VANILLA DOES NOT: {' '.join(extra)}")
    if stray:
        out.append("# SCENARIOS INVOKE COMMANDS NO DEDICATED SERVER REGISTERS: "
                   f"{' '.join(stray)}")
        out.append("# Those can only ever measure the unknown-command reply, which")
        out.append("# this server does not send (PARITY-ROADMAP 82.6).")
    out.append(HEADER)
    out += ["\t".join(r) for r in rows]
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--write", action="store_true")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--worklist", action="store_true")
    a = ap.parse_args()
    try:
        text = render()
    except Refusal as e:
        print(f"command-surface: REFUSED — {e}", file=sys.stderr)
        return 2
    if a.write:
        OUT.parent.mkdir(parents=True, exist_ok=True)
        OUT.write_text(text)
        print(f"wrote {OUT.relative_to(ROOT)}")
        return 0
    if a.worklist:
        for ln in text.split("\n"):
            if ln.startswith("#") or ln == HEADER or not ln:
                continue
            fs = ln.split("\t")
            if fs[4] == "no":
                print("\t".join([fs[0], fs[2], fs[5], fs[10]]))
        return 0
    if a.check:
        # Without the executed dump the rendering is a *different, weaker*
        # document: no corpus_reach column and no cross-check against a real
        # dispatcher. Comparing the committed file against it would either
        # fail for the wrong reason or, worse, pass while checking less than
        # the file was built from. Refuse and say which.
        if not DUMP.exists():
            print(f"command-surface: REFUSED — no {DUMP.relative_to(ROOT)}. Run "
                  "ci/command-surface-dump.sh; without it the executed derivation "
                  "cannot be compared and corpus_reach is unknown.", file=sys.stderr)
            return 2
        if not OUT.exists():
            print(f"command-surface: REFUSED — {OUT.relative_to(ROOT)} does not "
                  "exist; run --write", file=sys.stderr)
            return 2
        have = OUT.read_text()
        if have != text:
            print("command-surface: REFUSED — the generated surface has drifted "
                  f"from {OUT.relative_to(ROOT)}. Run ci/command-surface.py --write "
                  "and read the diff before committing it.", file=sys.stderr)
            import difflib
            sys.stderr.writelines(difflib.unified_diff(
                have.splitlines(True), text.splitlines(True),
                fromfile="committed", tofile="derived"))
            return 1
        print(f"command-surface: {OUT.relative_to(ROOT)} matches the jar")
        return 0
    ap.print_help()
    return 0


if __name__ == "__main__":
    sys.exit(main())
