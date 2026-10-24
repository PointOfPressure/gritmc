#!/usr/bin/env python3
"""entity-id-allocator.py — vanilla's entity id allocator, read from the jar,
against ours, read from this workspace.

# Why an allocator is a parity subject and not an implementation detail

`Mob.serverAiStep` re-evaluates its goal lists only when
`(tickCount + getId()) % 2 == 0` (or on the mob's first two ticks). The goal
phase is therefore a function of the entity's **id**, and nothing about the
entity's own state distinguishes a mob on the even phase from one on the odd
phase. Two servers that hand out ids in a different order run goals on
opposite ticks and no packet says so.

The requirement that falls out of that is narrower than "allocate the same
ids", and the narrowing is the useful part: `% 2` reads **one bit**. What has
to agree is the *parity* each entity is handed, which for a dense counter is
"the same number of draws happened before this entity was constructed". An
allocator that skips a number, or draws one for an object vanilla never
constructs, flips that bit for every entity after it.

# What this reads, and what it refuses to read

Vanilla comes from `javap -p -c -constants` on the pinned jar and nothing
else. Ours comes from the Rust sources in this tree. Both readers are checked
alive before anything is printed: a census that cannot find its anchors
reports "no difference" in exactly the same words as one that found none, and
this subject's headline is an ABSENCE (no divergence today, because nothing
observes it), which is the reading a dead reader produces for free.

Exit 3 with an empty stdout is the refusal. Exit 1 is a real difference under
`--check`. Exit 0 is agreement.
"""

import argparse
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
JAR_REL = "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
PIN = os.path.join(HERE, "checks", "entity-id-allocator.json")

SERVER_LEVEL = "net.minecraft.server.level.ServerLevel"
ENTITY = "net.minecraft.world.entity.Entity"
LEVEL = "net.minecraft.world.level.Level"

OURS_ALLOC = os.path.join(ROOT, "crates", "rustmc-server", "src", "entities.rs")


def die(msg):
    """Refuse. Nothing on stdout, because a refusal must not read as a result."""
    print("entity-id-allocator: REFUSED — %s" % msg, file=sys.stderr)
    sys.exit(3)


def jar_path():
    """The pinned jar. `RUSTMC_JAR` overrides it, which exists so the falsifier
    can hand this a file that is not a jar and watch the reader refuse."""
    p = os.environ.get("RUSTMC_JAR") or os.path.join(ROOT, JAR_REL)
    return p if os.path.exists(p) else None


def javap(jar, cls):
    try:
        out = subprocess.run(
            ["javap", "-p", "-c", "-constants", "-classpath", jar, cls],
            capture_output=True, text=True, timeout=180)
    except (OSError, subprocess.SubprocessError) as e:
        die("javap could not run (%s)" % e)
    if out.returncode != 0 or not out.stdout.strip():
        die("javap produced nothing for %s" % cls)
    return out.stdout


def method_body(text, signature):
    """The bytecode lines of one method, by its exact javap declaration line."""
    lines = text.splitlines()
    for i, ln in enumerate(lines):
        if ln.strip() == signature:
            body = []
            for j in range(i + 1, len(lines)):
                if lines[j].strip() == "" and body:
                    break
                body.append(lines[j])
            return "\n".join(body)
    return None


def read_vanilla(jar):
    """Vanilla's allocator, as a set of properties each anchored on a real line."""
    sl = javap(jar, SERVER_LEVEL)
    en = javap(jar, ENTITY)
    lv = javap(jar, LEVEL)

    alloc = method_body(sl, "public int getNextEntityId();")
    if alloc is None:
        die("ServerLevel.getNextEntityId not found — the allocator reader is dead")

    v = {}
    v["counter_type"] = "AtomicInteger" if re.search(
        r"private static final java\.util\.concurrent\.atomic\.AtomicInteger ENTITY_COUNTER;",
        sl) else None
    v["counter_is_static"] = bool(
        re.search(r"getstatic\s+#\d+\s+// Field ENTITY_COUNTER:"
                  r"Ljava/util/concurrent/atomic/AtomicInteger;", alloc))
    v["op_is_increment_and_get"] = bool(
        re.search(r"Method java/util/concurrent/atomic/AtomicInteger\."
                  r"incrementAndGet:\(\)I", alloc))
    # `i` starts at 0 and the loop head tests it, so the first call always
    # increments: there is no path that returns the counter's current value.
    v["starts_at_zero_and_pre_increments"] = bool(
        re.search(r"^\s*0: iconst_0\s*$", alloc, re.M)) and \
        bool(re.search(r"^\s*1: istore_1\s*$", alloc, re.M))
    v["skips_zero"] = bool(re.search(r"^\s*2: iload_1\s*$", alloc, re.M)) and \
        bool(re.search(r"^\s*3: ifeq ", alloc, re.M))
    v["skips_ids_live_in_this_level"] = bool(
        re.search(r"Method net/minecraft/server/level/ServerChunkCache\."
                  r"hasEntityWithId:\(I\)Z", alloc))

    # The counter's initial value. `new AtomicInteger()` takes the no-argument
    # constructor, whose initial value is 0, so the first id handed out is 1.
    static_init = method_body(sl, "static {};")
    if static_init is None:
        die("ServerLevel.<clinit> not found — cannot read the counter's initial value")
    v["counter_initialised_no_arg"] = bool(
        re.search(r"Method java/util/concurrent/atomic/AtomicInteger\.\"<init>\":\(\)V",
                  static_init))
    v["first_id"] = 1 if (v["counter_initialised_no_arg"]
                          and v["op_is_increment_and_get"]) else None

    # WHERE the draw happens. In the Entity constructor, so an entity that is
    # constructed and then discarded still consumes a number.
    ctor = method_body(
        en, "public net.minecraft.world.entity.Entity"
            "(net.minecraft.world.entity.EntityType<?>, net.minecraft.world.level.Level);")
    if ctor is None:
        die("Entity.<init> not found — cannot say where the id is drawn")
    v["drawn_in_entity_constructor"] = bool(
        re.search(r"Method net/minecraft/world/level/Level\.getNextEntityId:\(\)I", ctor))

    # Id 0 is not merely unallocated, it is a tripwire.
    getid = method_body(en, "public int getId();")
    if getid is None:
        die("Entity.getId not found")
    v["zero_is_a_tripwire"] = "Tried to access entity ID before ID assignment" in getid

    # The base Level returns 0, which is why the tripwire above is the client's
    # and the non-server level's behaviour rather than an impossible state.
    base = method_body(lv, "public int getNextEntityId();")
    if base is None:
        die("Level.getNextEntityId not found")
    v["base_level_returns_zero"] = bool(re.search(r"^\s*0: iconst_0\s*$", base, re.M))

    # THE CONSTRUCT-AND-DISCARD SITE, which is what makes "where the draw
    # happens" a parity subject rather than a curiosity. `PlayerList.respawn`
    # builds a second `ServerPlayer` — drawing an id in the constructor above —
    # and then stamps the old id over it. The drawn number is abandoned, so a
    # respawn advances the counter by one with nothing in the world to show
    # for it, and every entity created afterwards has the opposite parity to
    # a server that did not burn one.
    pl = javap(jar, "net.minecraft.server.players.PlayerList")
    v["respawn_burns_an_id"] = bool(re.search(
        r"Method net/minecraft/server/level/ServerPlayer\.setId:\(I\)V", pl))

    # THE CONSUMER, and the reason this whole subject is a parity subject.
    # `Mob.serverAiStep` offsets 36-49, read as opcodes rather than trusted
    # from §94's prose: tickCount, getId(), iadd, iconst_2, irem, ifeq.
    mob = javap(jar, "net.minecraft.world.entity.Mob")
    try:
        i = mob.index("  protected final void serverAiStep();")
        sas = mob[i:mob.index("\n\n", i)]
    except ValueError:
        die("Mob.serverAiStep not found — the goal-phase reader is dead")
    v["goal_phase_is_tickcount_plus_id_mod_2"] = bool(re.search(
        r"getfield\s+#\d+\s+// Field tickCount:I\s*\n"
        r"\s*\d+: aload_0\s*\n"
        r"\s*\d+: invokevirtual\s+#\d+\s+// Method getId:\(\)I\s*\n"
        r"\s*\d+: iadd\s*\n"
        r"\s*\d+: istore_2\s*\n"
        r"\s*\d+: iload_2\s*\n"
        r"\s*\d+: iconst_2\s*\n"
        r"\s*\d+: irem\s*\n"
        r"\s*\d+: ifeq ", sas))
    if not v["goal_phase_is_tickcount_plus_id_mod_2"]:
        die("Mob.serverAiStep no longer gates on (tickCount + getId()) %% 2; "
            "the reason entity id allocation is a parity subject at all has "
            "moved and this check must not answer from a stale pin")

    if not any(v[k] for k in ("counter_is_static", "op_is_increment_and_get",
                              "skips_ids_live_in_this_level")):
        die("no allocator property matched — the bytecode reader is dead")
    return v


def read_ours():
    """Ours, from the Rust source. Anchored on the real declaration, not a name."""
    if not os.path.exists(OURS_ALLOC):
        die("our allocator source is missing: %s" % OURS_ALLOC)
    src = open(OURS_ALLOC, encoding="utf-8").read()
    o = {}
    o["counter_is_static"] = bool(re.search(r"static SHARED: OnceLock<Entities>", src))
    o["counter_type"] = "AtomicI32" if re.search(r"next_id:\s*AtomicI32", src) else None
    body = re.search(r"pub fn next_id\(&self\) -> i32 \{(.*?)\n    \}", src, re.S)
    if body is None or o["counter_type"] is None:
        die("our allocator was not found in %s — the Rust reader is dead" % OURS_ALLOC)
    b = body.group(1)
    o["op_is_increment_and_get"] = bool(re.search(r"fetch_add\(1,\s*Ordering::Relaxed\)\s*\+\s*1", b))
    o["starts_at_zero_and_pre_increments"] = o["op_is_increment_and_get"]
    o["skips_zero"] = o["op_is_increment_and_get"]  # first value is 1; 0 is never returned
    o["skips_ids_live_in_this_level"] = bool(re.search(r"contains_key|live\.lock", b))
    o["counter_initialised_no_arg"] = bool(re.search(r"#\[derive\([^)]*Default[^)]*\)\]\s*\npub struct Entities", src))
    o["first_id"] = 1 if o["op_is_increment_and_get"] else None
    o["drawn_in_entity_constructor"] = None   # we have no Entity constructor
    o["zero_is_a_tripwire"] = None
    o["base_level_returns_zero"] = None
    # We implement no respawn at all: `crates/rustmc-server/src/menu.rs` names
    # it as a path this server does not have. Read rather than hard-coded, so
    # the day one lands this row moves on its own.
    src_all = ""
    srcdir = os.path.join(ROOT, "crates", "rustmc-server", "src")
    for name in sorted(os.listdir(srcdir)):
        if name.endswith(".rs"):
            src_all += open(os.path.join(srcdir, name), encoding="utf-8").read()
    #
    # **Keyed on the respawn HANDLER, `fn perform_respawn`, not on any
    # `fn respawn`** — 2026-09-30, `damage`: that branch added
    # `PlayerMovement::respawn` (the movement half) and the old
    # `fn\s+respawn\b` read that as "a respawn exists and burns nothing",
    # False, while the handler that constructs the new player is elsewhere.
    # Read from the handler's own body: it burns an id iff it draws one.
    # (`crate::respawn` has a `perform_respawn` too — the pure health gate —
    # so the handler is the one that takes the `Session`.)
    m = re.search(r"fn perform_respawn\(\s*s: &mut Session(.*?)\n}\n", src_all, re.S)
    o["respawn_burns_an_id"] = None if m is None else bool(
        re.search(r"next_id\(\)", m.group(1)))
    # THE CONSUMER'S REACHABILITY HERE. The goal phase needs a `Mob`, and our
    # whole entity vocabulary is the `EntityKind` constants in this file.
    #
    # **"A third one lands" IS NOT "a mob lands", and this read it as though it
    # were until 2026-09-05.** `mob_kinds` below was
    # `[k for k in entity_kinds if k not in ("PLAYER", "ITEM")]` — an allowlist
    # frozen at the moment the file was written, when the only plausible third
    # constant was a mob. `stack-gameplay`'s `FALLING_BLOCK` is the third
    # constant and is `MobCategory.MISC`, so the check refused a tree whose
    # standing note is still exactly true, naming the branch while measuring
    # its own allowlist. The prose one paragraph down had the criterion right
    # all along — "whose intersection with the jar's `Mob` subtypes is empty" —
    # and so did `ci/entity-id-allocator-falsify.sh` arm 5, which injects
    # `name: "minecraft:pig"` rather than a bare constant. Only the code
    # disagreed.
    #
    # So the registry id is read beside the constant name and intersected with
    # the jar's own non-MISC set, which is what `ci/mob-tick-census.py` already
    # derives and caches. Measured, not listed: the day `minecraft:pig` lands
    # this refuses, and the day a second falling block lands it does not.
    kinds = []
    for m in re.finditer(
            r"^pub const ([A-Z_][A-Z0-9_]*): EntityKind = EntityKind \{", src, re.M):
        # Bounded by the struct literal's own `};`: a fixed-width window
        # spilled into the NEXT constant once a kind followed `EXPERIENCE_ORB`
        # (whose name is a path, not a literal), and read the golem's id as
        # the orb's (2026-09-30, entity-ports).
        end = src.find("};", m.end())
        tail = src[m.end():end if end != -1 else m.end() + 400]
        rid = re.search(r'name:\s*"([^"]+)"', tail)
        if rid is None:
            # `name: crate::<module>::<CONST>` — `EXPERIENCE_ORB` names its
            # registry id through `crate::xporb::ORB_NAME`. Resolved from that
            # module's own `const <CONST>: &str = "..."`; left `None` (which
            # `our_mob_kinds` counts as a mob, the safe direction) if not.
            path = re.search(r"name:\s*crate::([a-z_][a-z0-9_]*)::([A-Z_][A-Z0-9_]*)", tail)
            if path:
                mod_src = os.path.join(os.path.dirname(OURS_ALLOC), path.group(1) + ".rs")
                if os.path.exists(mod_src):
                    rid = re.search(r"const\s+%s:\s*&str\s*=\s*\"([^\"]+)\"" % path.group(2),
                                    open(mod_src, encoding="utf-8").read())
        kinds.append((m.group(1), rid.group(1) if rid else None))
    o["entity_kinds"] = sorted(k for k, _ in kinds)
    o["entity_kind_ids"] = {k: r for k, r in sorted(kinds)}
    o["goal_phase_is_tickcount_plus_id_mod_2"] = None
    return o


def our_mob_kinds(o):
    """Our `EntityKind` constants that really are mobs, by registry id.

    The jar side comes from `ci/mob-tick-census.py`'s `mob_types` — the
    `EntityTypes` static initialiser's entries whose `MobCategory` is not
    `MISC` — which that script derives once per jar and caches. It runs
    earlier in `ci/t0.sh` than this does, so the cache is normally warm; a
    cold one costs one javap pass and is still correct.

    A constant whose registry id could not be read is treated as a mob. That
    is the safe direction: this function gates a REFUSAL that says "the
    standing note has gone stale", and failing to read an id must not be able
    to keep the note alive.
    """
    import importlib.util
    spec = importlib.util.spec_from_file_location(
        "mob_tick_census", os.path.join(HERE, "mob-tick-census.py"))
    mod = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(mod)
        mob_types = set(mod.jar_figures(jar_path())["mob_types"])
    except Exception as e:                       # noqa: BLE001 - reported, not swallowed
        die("could not read the jar's Mob set through ci/mob-tick-census.py "
            "(%s), so whether any of our EntityKinds is a mob is unknown and "
            "the standing refusal cannot be adjudicated" % e)
    ids = o.get("entity_kind_ids", {})
    return [k for k in o["entity_kinds"]
            if ids.get(k) is None or ids[k] in mob_types]


# The properties that are a PARITY REQUIREMENT, with why. A property not on
# this list is read and reported but does not make the check red: the check
# has to be able to say "different and it does not matter" without either
# lying or going quiet.
REQUIRED = {
    "op_is_increment_and_get":
        "the counter pre-increments, so the first id is 1 and no entity is ever 0",
    "starts_at_zero_and_pre_increments":
        "same draw sequence, so the same entity gets the same parity bit",
    "skips_zero":
        "0 is the wire's no-entity sentinel and vanilla's own tripwire",
    "first_id":
        "the first entity in a session is 1 on both sides or every parity bit is flipped",
}

# Properties that DIFFER and are recorded as differing on purpose, each with
# the reason it is not a bug today and what would make it one.
KNOWN_DIFFERENT = {
    "skips_ids_live_in_this_level":
        "vanilla loops until `ChunkMap.entityMap` has no such id; ours does not. "
        "Under a monotonic counter the loop body is unreachable until the "
        "counter wraps at 2^31, so this is a wraparound guard we do not need "
        "yet. It becomes a divergence the moment either side reuses ids.",
    "counter_is_static":
        "both are process-wide, but vanilla's is static on ServerLevel and "
        "shared across every dimension while ours is a process-wide OnceLock. "
        "Same observable sequence; different owner.",
    "goal_phase_is_tickcount_plus_id_mod_2":
        "read from the jar and not from ours: ours is transcribed at "
        "crates/rustmc-entity/src/ai/mob_ai.rs:113, and this reader does not "
        "parse Rust control flow, so the row is None rather than a claim. It is "
        "the CONSUMER — the reason the allocator is a parity subject at all — "
        "and it is verified as an opcode sequence rather than trusted from "
        "prose: tickCount, getId(), iadd, iconst_2, irem, ifeq at "
        "Mob.serverAiStep offsets 36-49. If it ever stops matching, this "
        "check refuses rather than carrying on grading an allocator nothing "
        "reads.",
    "entity_kinds":
        "our whole entity vocabulary. Counted rather than asserted so the "
        "`NOT GRADED` row above moves on its own the day a third kind lands.",
    "counter_type":
        "`AtomicInteger` against `AtomicI32`: the same 32-bit width and the "
        "same wrap point, which is the only property of the type the wire can "
        "see. A u64 counter would NOT be equivalent — the wire field is a "
        "varint-encoded i32 and the wrap is observable.",
    "zero_is_a_tripwire":
        "vanilla's `Entity.getId` throws `Tried to access entity ID before ID "
        "assignment` on a zero id. Ours cannot reach the state — an id is a "
        "value handed out at construction of the record, never a default — so "
        "there is nothing to throw about. Recorded because it is the reason 0 "
        "is skipped rather than merely unused.",
    "base_level_returns_zero":
        "`Level.getNextEntityId` returns 0 and `ServerLevel` overrides it. "
        "That is the client's and the non-server level's answer, and it is "
        "what the tripwire above is for. We have no non-server level.",
    "respawn_burns_an_id":
        "vanilla's `PlayerList.respawn` constructs a second ServerPlayer, "
        "which draws an id, then stamps the old id over it — one number burned "
        "per respawn with nothing in the world holding it. "
        "`world_send::perform_respawn` burns one the same way (since "
        "`damage`, 2026-09-30).",
    "drawn_in_entity_constructor":
        "vanilla draws in `Entity.<init>`, so an entity constructed and then "
        "discarded still burns a number — `PlayerList.respawn` does exactly "
        "that. We have no entity constructor and draw at the two call sites "
        "that make an entity. Nothing observes the difference today because "
        "we implement no path that constructs-and-discards; a respawn that "
        "did not burn one would be the first.",
}


def mob_behaviour_checks():
    """Names of mob-bearing checks whose `observe` carries a behaviour channel.

    Read from `ci/checks/mob-blind.toml`; a missing ledger is a refusal, not an
    empty list, because an empty list is the claim "graded nowhere".
    """
    import tomllib
    path = os.path.join(ROOT, "ci", "checks", "mob-blind.toml")
    if not os.path.exists(path):
        die("no %s — cannot say which mob checks observe behaviour" % path)
    with open(path, "rb") as fh:
        rows = tomllib.load(fh).get("blind", [])
    if not rows:
        die("%s lists no mob-bearing check" % path)
    return sorted(r["name"] for r in rows if r.get("observes_behaviour"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true",
                    help="refuse if a required property differs or a pinned figure moved")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--rewrite-pin", action="store_true")
    args = ap.parse_args()

    jar = jar_path()
    if jar is None:
        die("no pinned jar under %s" % os.path.join(ROOT, JAR_REL))

    v = read_vanilla(jar)
    o = read_ours()

    rows = []
    for k in sorted(set(v) | set(o)):
        rows.append((k, v.get(k), o.get(k)))

    mismatched_required = [k for k in REQUIRED if v.get(k) != o.get(k)]

    # WHAT THIS CHECK REFUSES TO GRADE, AND WHY IT SAYS SO OUT LOUD.
    #
    # The allocator agreeing is not the goal phase agreeing. The goal phase
    # needs a `Mob`, and `ci/mob-tick-census.py` establishes that the
    # intersection of our `EntityKind` set with the jar's 114 Mob subtypes is
    # EMPTY. So the consumer cannot run here and no green from this file is
    # evidence about it.
    #
    # This is stated as a field rather than left implicit because a check that
    # passes over a subject that cannot occur is this project's central defect
    # class, and the cheapest form of it is a true headline that a reader
    # takes for a wider one.
    mob_kinds = our_mob_kinds(o)
    if not mob_kinds:
        ungraded = {
            "subject": "Mob.serverAiStep's goal phase",
            "reachable_here": False,
            "why": ("our EntityKind set is %s, whose intersection with the jar's "
                    "Mob subtypes is empty, so no mob can exist in this server and "
                    "the goal phase cannot run. The allocator below is graded; the "
                    "goal phase is NOT, and this check refuses to be read as "
                    "evidence about it." % ", ".join(o["entity_kinds"])),
            "measured_against": "the jar's non-MISC EntityType registry ids, via "
                                "ci/mob-tick-census.py, not a list of names",
        }
    else:
        # **THE REFUSAL WENT STALE ON `mob-in-world`, 2026-09-24, AS IT WAS
        # BUILT TO.** Until then this branch died here: a mob kind made the
        # "no mob can exist" row false, and a false row must not be pinnable.
        # The row is now derived rather than asserted — which kinds are mobs,
        # and which of the corpus's mob-bearing checks observe any behaviour
        # channel at all, read from `ci/checks/mob-blind.toml` (written by
        # `ci/mob-observability.py --ledger`). Both are in the pinned report,
        # so a new mob kind or the first behaviour-observing mob check moves
        # the pin and reddens `--check` until someone re-reads this row; and
        # the goal phase is still not graded HERE, which is said out loud.
        behaviour = mob_behaviour_checks()
        ungraded = {
            "subject": "Mob.serverAiStep's goal phase",
            "reachable_here": True,
            "mob_kinds": mob_kinds,
            "measured_against": "the jar's non-MISC EntityType registry ids, via "
                                "ci/mob-tick-census.py, not a list of names",
            "behaviour_observing_mob_checks": behaviour,
            "why": ("%s %s mob kind(s), so the goal phase runs in this server. "
                    "This check grades the allocator only. Of the corpus's "
                    "mob-bearing checks, %s observe a behaviour channel "
                    "(ci/checks/mob-blind.toml), so the goal phase's "
                    "`(tickCount + getId()) %% 2` interleaving is %s."
                    % (", ".join(mob_kinds),
                       "is a" if len(mob_kinds) == 1 else "are",
                       ", ".join(behaviour) if behaviour else "none",
                       "exercised by those checks and NOT graded here"
                       if behaviour else "graded NOWHERE")),
        }

    report = {"vanilla": v, "ours": o, "ungraded": ungraded,
              "required_mismatch": sorted(mismatched_required)}

    if args.rewrite_pin:
        with open(PIN, "w", encoding="utf-8") as fh:
            json.dump(report, fh, indent=2, sort_keys=True)
            fh.write("\n")
        print("rewrote %s" % PIN)
        return 0

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0

    print("entity id allocator — vanilla (pinned jar) against ours")
    print()
    print("  %-38s %-22s %s" % ("property", "vanilla", "ours"))
    for k, a, b in rows:
        mark = ""
        if a != b:
            mark = "  <- REQUIRED" if k in REQUIRED else "  <- known, see the ledger"
        print("  %-38s %-22s %s%s" % (k, a, b, mark))
    print()
    print("vanilla, transcribed:")
    print("    int i = 0;")
    print("    while (i == 0 || this.chunkSource.hasEntityWithId(i))")
    print("        i = ENTITY_COUNTER.incrementAndGet();")
    print("    return i;")
    print()
    print("ours:  self.next_id.fetch_add(1, Ordering::Relaxed) + 1")
    print()
    print("  NOT GRADED — %s" % ungraded["subject"])
    print("      %s" % ungraded["why"])
    print()
    for k, why in sorted(KNOWN_DIFFERENT.items()):
        if v.get(k) != o.get(k):
            print("  known difference — %s:\n      %s" % (k, why))

    if args.check:
        if not os.path.exists(PIN):
            die("no pin at %s; run --rewrite-pin once and commit it" % PIN)
        pinned = json.load(open(PIN, encoding="utf-8"))
        if pinned != report:
            print("entity-id-allocator: the allocator moved against its pin", file=sys.stderr)
            for side in ("vanilla", "ours"):
                for k in sorted(set(pinned[side]) | set(report[side])):
                    if pinned[side].get(k) != report[side].get(k):
                        print("   %s.%s: pinned %r, now %r"
                              % (side, k, pinned[side].get(k), report[side].get(k)),
                              file=sys.stderr)
            return 1
        if mismatched_required:
            print("entity-id-allocator: required property differs: %s"
                  % ", ".join(mismatched_required), file=sys.stderr)
            return 1
        print("entity-id-allocator: %d properties read, %d required, all agree"
              % (len(rows), len(REQUIRED)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
