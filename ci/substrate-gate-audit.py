#!/usr/bin/env python3
"""substrate-gate-audit.py — what the corpus cannot grade because of its own substrate.

    ./ci/substrate-gate-audit.py
    ./ci/substrate-gate-audit.py --recordings '~/rustmc*/oracle-work/gameplay/vanilla.json'
    ./ci/substrate-gate-audit.py --gates-only        # no recordings needed

`ci/corpus-surface.py` answers *how much of the protocol the corpus can see*.
`ci/unported-inventory.py` answers *what this server has not written*. Neither
answers the question this script exists for, and the two get confused with each
other constantly:

    a kind nobody has ever recorded is not evidence of a missing port.
    It may be evidence that VANILLA never sent it either, because the
    substrate the corpus runs on takes an early return before the send.

That distinction is the whole content here. `damage_event` sat in the roadmap
as an unported work item until 2026-08-18, when it turned out that
`Player.hurtServer` reads `abilities.invulnerable` at offset 15 and returns
false at 31-32 on a creative player, and **no server-side work could ever have
moved it**. (The entity lane reported this as "offset 31", which is the
`iconst_0`; the `ireturn` is 32 and the gate itself is 15. Stated once here so
nobody re-derives it.) This script is the instrument that
would have said so, and it is written so that the next such row is refused
before it is written rather than after an agent has spent a turn on it.

# THE SUBSTRATE IS THREE LEVERS, NOT ONE

`ci/t2-gameplay.sh` boots the reference with

    gamemode=creative  difficulty=peaceful  spawn-monsters=false
    generate-structures=false  spawn-radius=0

and four game rules — `advance_time false`, `spawn_mobs false`,
`random_tick_speed 0`, `advance_weather false`. Of those, **three** gate code
that a parity check would want to grade, and they gate *different* things:

  * **`gamemode=creative`** sets `Abilities.invulnerable` and
    `Abilities.instabuild`. This is the lever everybody names.
  * **`difficulty=peaceful`** is a SECOND lever inside the same method, and it
    was not named anywhere before this branch. `Player.hurtServer` offsets
    51-115 zero the damage and return false for any source whose
    `DamageScaling` is `always` — independent of game mode. It also stops
    `FoodData.tick` decrementing the food level at all.
  * **the game rules** are already declarable per scenario, via
    `scenario::GameRule`. They are named here only so that a reader does not
    attribute a rule-shut mechanism to the game mode.

A finding that names the wrong lever is worse than no finding, because flipping
the named one leaves the mechanism just as dark and the next reader concludes
the mechanism is broken. Every row below therefore carries its lever.

# WHY THE GATES ARE DECLARED AND THEN VERIFIED, RATHER THAN DISCOVERED

A general "find every branch on a creative flag" pass over 7,445 classes is a
call-graph problem, and a partial call graph that reads complete is this
project's most expensive defect class. So `GATES` below is **hand-read**, each
row carrying the class, the method, the offset and the shape — and this script
does not trust one word of it. It re-reads the pinned jar with
`javap -p -c -constants` and refuses if any row no longer describes the
bytecode. A row that rots is a red, not a stale comment.

That makes this a falsifiable reading rather than an enumeration, and the
header says which it is. What it CANNOT tell you is whether some further gate
exists that nobody has read. `--gates-only` prints the declared set and its
verification; it never claims the set is closed.

# THE JOIN, AND THE ONE CLAIM IT IS ALLOWED TO MAKE

For each kind in `trace::DECODED_KINDS` that no recording has ever carried,
this prints the sender it was hand-traced to and the gate above it. The claim
is precisely: *vanilla did not send this, and here is the return that stopped
it*. It is NOT "this would be green if the substrate changed" — nothing here
knows what this server would do, and `ci/unported-inventory.py` is the
instrument for that half. The two populations OVERLAP — `damage-path` is a GAP
marker in that inventory *and* substrate-blocked here — so a row appearing in
one is never evidence about the other, and the footer says so every run.

# THE COUNTS ARE OVER RECORDINGS, DEDUPED, AND N IS PRINTED

A kind's absence from one recording is nothing. The recordings on this box
include copies of each other, so they are deduped by sha256 before counting and
the surviving N is printed next to every zero. A zero without its N is not a
finding and this script will not print one.

Reading the recordings WITHOUT the projection is deliberate and is sound only
because every count here is a presence census of **one server at a time** —
never a comparison of two streams. That is the same basis on which
`ci/entity-reach.py`, `ci/entity-leak-audit.py` and `ci/corpus-dead-observe.py`
read raw traces; `docs/cold-start/corpus-projection-audit.md` covers the other
kind, where unprojected screens manufactured hits that dissolved.

This script gates nothing on its own. `ci/substrate-gate-audit-falsify.sh` is
what proves it can still fail.

# IT IS NOT REGISTERED IN T0, AND THAT IS A DECISION RATHER THAN AN OMISSION

The falsifier takes ~23 s — it invokes this script eight times and each one
opens a 25 MB jar. That was 61 s until the extraction was narrowed from
`net/minecraft/*` (7,445 classes) to the six this script actually reads.

`docs/CI.md` records that T0 grew 17 s on 2026-08-18 against a budget measured
that morning, and a lane is actively managing it. Adding 23 s to buy a check on
a *reading* — one that changes only when Mojang ships a new jar, which is
pinned — is a poor trade at that price. So this follows the precedent of
`ci/entity-reach.py`'s siblings: four of that branch's six tools print and
never gate, each saying so in its own header.

**Run the falsifier by hand when the jar pin moves.** That is the only event
that can rot these rows, and it is not an event a per-commit gate is shaped to
catch.
"""

import argparse
import collections
import glob
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JAR = os.path.join(ROOT, "oracle-work", "vanilla-26.2", "versions", "26.2", "server-26.2.jar")
TRACE_RS = os.path.join(ROOT, "tools", "oracle-runner", "src", "trace.rs")
DEFAULT_RECORDINGS = [
    os.path.expanduser("~/grit/**/vanilla.json"),
    os.path.expanduser("~/rustmc*/**/vanilla.json"),
]

# The levers, by the name the substrate sets them with.
LEVER_GAMEMODE = "gamemode=creative"
LEVER_DIFFICULTY = "difficulty=peaceful"


def die(msg):
    print(f"substrate-gate-audit: {msg}", file=sys.stderr)
    sys.exit(2)


# --------------------------------------------------------------------------
# The declared gates.
#
# `needle` is a substring that must appear in the method's disassembly for the
# row to still hold; `shape` is what a reader should see there. `kind` is
# `early-return` when the gated branch reaches a return without doing anything
# observable, `suppress` when the gate merely stops a value changing.
# --------------------------------------------------------------------------
GATES = [
    dict(
        cls="net.minecraft.world.entity.player.Player",
        method="hurtServer",
        gate_at=15, return_at=32,
        lever=LEVER_GAMEMODE,
        kind="early-return",
        needle="Field net/minecraft/world/entity/player/Abilities.invulnerable:Z",
        shape="abilities.invulnerable && !source.is(BYPASSES_INVULNERABILITY) -> return false",
        blocks="damage_event, set_health, hurt_animation, entity_event(damage)",
    ),
    dict(
        cls="net.minecraft.world.entity.player.Player",
        method="hurtServer",
        gate_at=62, return_at=115,
        lever=LEVER_DIFFICULTY,
        kind="early-return",
        needle="Field net/minecraft/world/Difficulty.PEACEFUL",
        shape="scalesWithDifficulty() && difficulty==PEACEFUL -> amount=0 -> return false",
        blocks="damage_event for the 4 `always`-scaling types (explosion, "
        "player_explosion, bad_respawn_point, sonic_boom)",
    ),
    dict(
        cls="net.minecraft.world.entity.player.Player",
        method="causeFoodExhaustion",
        gate_at=4, return_at=10,
        lever=LEVER_GAMEMODE,
        kind="early-return",
        needle="Field net/minecraft/world/entity/player/Abilities.invulnerable:Z",
        shape="abilities.invulnerable -> return",
        blocks="set_health.food, set_health.saturation",
    ),
    dict(
        cls="net.minecraft.world.food.FoodData",
        method="tick",
        gate_at=58, return_at=None,
        lever=LEVER_DIFFICULTY,
        kind="suppress",
        needle="Field net/minecraft/world/Difficulty.PEACEFUL",
        shape="difficulty==PEACEFUL -> skip the foodLevel decrement",
        blocks="set_health.food",
    ),
    dict(
        cls="net.minecraft.server.level.ServerPlayerGameMode",
        method="handleBlockBreakAction",
        gate_at=178, return_at=194,
        lever=LEVER_GAMEMODE,
        kind="early-return",
        needle="Field net/minecraft/world/entity/player/Abilities.instabuild:Z",
        shape="abilities.instabuild -> destroyAndAck(pos, seq, 'creative destroy'); return "
        "-- BEFORE destroyProgressStart/destroyPos are assigned",
        blocks="block_destruction, and every STOP_DESTROY_BLOCK path below it",
    ),
    dict(
        cls="net.minecraft.server.level.ServerPlayerGameMode",
        method="destroyBlock",
        gate_at=193, return_at=200,
        lever=LEVER_GAMEMODE,
        kind="early-return",
        needle="Method net/minecraft/server/level/ServerPlayer.preventsBlockDrops:()Z",
        shape="player.preventsBlockDrops() -> return true, BEFORE Block.playerDestroy",
        blocks="the dig-drop route to add_entity(item), take_item_entity, set_experience",
    ),
    dict(
        cls="net.minecraft.world.entity.player.Player",
        method="preventsBlockDrops",
        gate_at=4, return_at=7,
        lever=LEVER_GAMEMODE,
        kind="suppress",
        needle="Field net/minecraft/world/entity/player/Abilities.instabuild:Z",
        shape="return abilities.instabuild -- the whole method",
        blocks="(the predicate the row above reads)",
    ),
]

# Kinds hand-traced to their sender. `sender` must be the class that
# constructs the packet; verified against the jar.
SENDERS = {
    "damage_event": ("net.minecraft.server.level.ServerLevel", "broadcastDamageEvent",
                     "ClientboundDamageEventPacket"),
    "block_destruction": ("net.minecraft.server.level.ServerLevel", "destroyBlockProgress",
                          "ClientboundBlockDestructionPacket"),
    "set_health": ("net.minecraft.server.level.ServerPlayer", "doTick",
                   "ClientboundSetHealthPacket"),
    "set_experience": ("net.minecraft.server.level.ServerPlayer", "doTick",
                       "ClientboundSetExperiencePacket"),
}

# Why each never-observed kind is never observed. `substrate` rows are the
# finding; `unasked` rows are a coverage gap and belong to a different lane.
ATTRIBUTION = {
    "damage_event": ("substrate", LEVER_GAMEMODE, "Player.hurtServer offset 31"),
    "set_health": ("substrate", LEVER_GAMEMODE, "nothing changes health or food; the send "
                   "in ServerPlayer.doTick is an unconditional change-detector"),
    "block_destruction": ("substrate", LEVER_GAMEMODE,
                          "ServerPlayerGameMode.handleBlockBreakAction offset 194"),
    "set_experience": ("substrate", LEVER_GAMEMODE,
                       "ServerPlayerGameMode.destroyBlock offset 200 (no drops, no orbs); "
                       "superflat also carries no ore, so this row is over-determined"),
    "game_event": ("both", LEVER_GAMEMODE,
                   "weather is shut by the `advance_weather false` game rule, and the other "
                   "reachable producer is the CHANGE_GAME_MODE event itself, which a corpus "
                   "that never changes game mode cannot provoke"),
    "player_position": ("unasked", None,
                        "a teleport/resync; no scenario provokes one. NOT substrate"),
    "set_default_spawn_position": ("unasked", None,
                                   "sent during login, before the recorder attaches. NOT substrate"),
}


# Every spelling a CREATIVE-LEVER gate takes in this jar, each paired with a
# method known to contain it. The pairing is the point: a needle that stops
# matching its own witness is a needle that has quietly stopped detecting, and
# every "no gate here" row below rests on these needles matching.
#
# `GameType` is DELIBERATELY ABSENT, and that absence is load-bearing.
# `ServerPlayerGameMode.useItem` opens with
#
#     0: getfield gameModeForPlayer:GameType
#     4: getstatic GameType.SPECTATOR
#     7: if_acmpne 14
#    10: getstatic InteractionResult.PASS
#    13: areturn
#
# which is a game-mode gate on the SPECTATOR lever. Including `GameType` would
# make that a false positive and mark `use_item` substrate-blocked, which it is
# not: spectator is a mode this server refuses to serve at all
# (`status::set_game_type`), and it is the same lever as `mayBuild`, not the
# creative one this lane owns.
GATE_SPELLINGS = {
    "Field net/minecraft/world/entity/player/Abilities":
        ("net.minecraft.server.level.ServerPlayerGameMode", "handleBlockBreakAction"),
    "instabuild":
        ("net.minecraft.server.level.ServerPlayerGameMode", "handleBlockBreakAction"),
    "invulnerable":
        ("net.minecraft.world.entity.player.Player", "hurtServer"),
    "isCreative":
        ("net.minecraft.server.level.ServerPlayerGameMode", "isCreative"),
    "preventsBlockDrops":
        ("net.minecraft.server.level.ServerPlayerGameMode", "destroyBlock"),
}

# The harness verbs `ci/corpus-coverage.py` reports as exercised by nothing,
# and whether the substrate is why. A verb at zero looks identical in that
# report whichever the reason is, and the two want opposite responses: one
# needs a scenario, the other needed a game mode.
#
# `gated` rows assert a gate EXISTS; `ungated` rows assert that NO creative
# -lever gate does — checked against every spelling in `GATE_SPELLINGS`, not
# against one string, because that is the direction that rots silently.
VERBS = {
    "finish_dig": dict(
        cls="net.minecraft.server.level.ServerPlayerGameMode",
        method="handleBlockBreakAction",
        gated=True,
        gate_needle="Field net/minecraft/world/entity/player/Abilities.instabuild:Z",
        note="STOP_DESTROY_BLOCK cannot match destroyPos, which the instabuild "
             "branch returns above. Needed a game mode, not a scenario.",
    ),
    "use_item": dict(
        cls="net.minecraft.server.level.ServerPlayerGameMode",
        method="useItem",
        gated=False,
        gate_needle=None,   # checked against every spelling instead
        note="no CREATIVE-lever gate in its 95 instructions. It does open with "
             "a SPECTATOR early return (offset 13), which is a different lever "
             "and one this server refuses to serve — so this zero is a COVERAGE "
             "gap and a scenario closes it. Not this lane's.",
    ),
}


def javap(workdir, cls):
    r = subprocess.run(
        ["javap", "-p", "-c", "-constants", "-cp", workdir, cls],
        capture_output=True, text=True,
    )
    if r.returncode != 0:
        die(f"javap {cls} failed — is this JDK 25? {r.stderr.strip()[:200]}")
    return r.stdout


def method_body(disasm, method):
    """The disassembly of one method, by name, as a list of (offset, text)."""
    out, inside = [], False
    for line in disasm.splitlines():
        if re.match(r"^  \S.*\b" + re.escape(method) + r"\(", line):
            inside = True
            continue
        if inside and re.match(r"^  \S", line) and "Code:" not in line:
            break
        if inside:
            m = re.match(r"^\s+(\d+): (.*)$", line)
            if m:
                out.append((int(m.group(1)), m.group(2)))
    return out


def decoded_kinds():
    try:
        src = open(TRACE_RS).read()
    except OSError as e:
        die(f"cannot read {TRACE_RS}: {e}")
    m = re.search(r"pub const DECODED_KINDS: &\[&str\] = &\[(.*?)\];", src, re.S)
    if not m:
        die("DECODED_KINDS not found in trace.rs — the list moved; fix this script")
    return re.findall(r'"([a-z_]+)"', m.group(1))


def load_recordings(patterns):
    files = []
    for p in patterns:
        files.extend(glob.glob(p, recursive=True))
    seen = {}
    for f in sorted(set(files)):
        try:
            with open(f, "rb") as fh:
                h = hashlib.sha256(fh.read()).hexdigest()
        except OSError:
            continue
        seen.setdefault(h, f)
    return seen


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recordings", action="append", default=None)
    ap.add_argument("--gates-only", action="store_true")
    ap.add_argument("--jar", default=JAR)
    args = ap.parse_args()

    if not shutil.which("javap"):
        die("javap not on PATH — JDK 25 is required (see docs/AGENT-BRIEF.md)")
    if not os.path.exists(args.jar):
        die(f"pinned jar absent at {args.jar} — provision oracle-work first")

    print("== substrate gates, re-read from the pinned jar ==\n")
    reds = 0
    with tempfile.TemporaryDirectory() as td:
        # Only the classes this script actually disassembles, not all 7,445.
        #
        # Extracting `net/minecraft/*` wholesale cost ~8 s per invocation, and
        # the falsifier invokes this seven times — 61 s for a tool that reads
        # six classes. javap does not resolve dependencies to disassemble, so
        # the named classes alone are enough.
        wanted = {g["cls"] for g in GATES}
        wanted |= {cls for cls, _, _ in SENDERS.values()}
        wanted.add("net.minecraft.world.level.GameType")
        names = {c.replace(".", "/") + ".class" for c in wanted}
        with zipfile.ZipFile(args.jar) as z:
            have = set(z.namelist())
            missing = names - have
            if missing:
                die("the pinned jar does not contain: "
                    + ", ".join(sorted(missing))
                    + " — a class this script reads has been renamed or removed")
            z.extractall(td, sorted(names))
        cache = {}
        print("%-44s %-8s %-9s %s"
              % ("class.method  gate@ -> return@", "verdict", "lever", "shape"))
        for g in GATES:
            key = g["cls"]
            if key not in cache:
                cache[key] = javap(td, key)
            body = method_body(cache[key], g["method"])
            off = dict(body)
            # the gate itself: the needle must be at the declared offset
            verdict = "ok" if g["needle"] in off.get(g["gate_at"], "") else (
                "MOVED" if any(g["needle"] in t for _, t in body) else "GONE")
            # and the branch it guards must still return where the row says
            if verdict == "ok" and g["return_at"] is not None:
                if not re.match(r"^\w*return\b", off.get(g["return_at"], "")):
                    verdict = "NO-RETURN"
            if verdict != "ok":
                reds += 1
            short = g["cls"].rsplit(".", 1)[-1]
            where = f"{short}.{g['method']}  {g['gate_at']} -> " + (
                str(g["return_at"]) if g["return_at"] is not None else "(none)")
            print("%-44s %-8s %-9s %s"
                  % (where, verdict, g["lever"].split("=")[1], g["shape"][:64]))
        print()
        for g in GATES:
            short = g["cls"].rsplit(".", 1)[-1]
            print(f"  {short}.{g['method']}@{g['gate_at']}  [{g['kind']}]  blocks: {g['blocks']}")

        print("\n== packet senders, verified present in the jar ==\n")
        # The NAMED packet class, not merely "some packet". Checking only that
        # a `new ...Packet` appears would accept any method that happens to
        # build one — and `ServerPlayer.doTick` builds several, so two of the
        # four rows below would have passed while pointing at the wrong send.
        for kind, (cls, meth, pkt) in sorted(SENDERS.items()):
            if cls not in cache:
                cache[cls] = javap(td, cls)
            body = method_body(cache[cls], meth)
            found = any(pkt in t and "new " in t for _, t in body)
            if not found:
                reds += 1
            print("  %-20s <- %-28s %s   %s"
                  % (kind, f"{cls.rsplit('.', 1)[-1]}.{meth}", pkt,
                     "ok" if found else "NOT CONSTRUCTED HERE"))

        # --- the ability matrix, because naming the wrong lever is the
        # failure this whole script exists to prevent -----------------------
        #
        # `mayBuild` reads like a creative flag and is not one: it is true in
        # SURVIVAL as well, and false only in ADVENTURE and SPECTATOR. So the
        # `maybuild-usewithoutitem` GAP in
        # `ci/checks/server-unported-inventory.json` is NOT opened by
        # `--gamemode survival` — it needs a mode this server deliberately
        # refuses. A lane that flipped the game mode expecting that marker to
        # become gradeable would find it exactly as dark as before, which is
        # the specific wasted turn this section prevents.
        print("\n== GameType.updatePlayerAbilities, read from the jar ==\n")
        gt = javap(td, "net.minecraft.world.level.GameType")
        ub = method_body(gt, "updatePlayerAbilities")
        # Each `iconst_N; putfield Abilities.<field>` pair, in bytecode order.
        assigns = []
        prev = None
        for _, txt in ub:
            m = re.search(r"Field net/minecraft/world/entity/player/Abilities\.(\w+):Z", txt)
            if m and prev is not None:
                assigns.append((m.group(1), prev))
            prev = 1 if txt.startswith("iconst_1") else (0 if txt.startswith("iconst_0") else None)
        if not assigns:
            print("  COULD NOT READ — updatePlayerAbilities has no iconst/putfield pairs")
            reds += 1
        else:
            # The three arms appear in GameType's own $VALUES order. Report
            # what was read rather than asserting an order the jar may change.
            print("  bytecode order of (field, value) assignments:")
            print("   ", ", ".join(f"{k}={v}" for k, v in assigns))
            inv = [v for k, v in assigns if k == "invulnerable"]
            ins = [v for k, v in assigns if k == "instabuild"]
            print(f"\n  invulnerable is set {inv} across the arms — 0 in exactly "
                  f"{inv.count(0)} of {len(inv)}")
            print(f"  instabuild   is set {ins} across the arms — 1 in exactly "
                  f"{ins.count(1)} of {len(ins)}")
            print("\n  mayBuild: assigned conditionally, NOT per-arm — it is true in "
                  "SURVIVAL and CREATIVE\n  alike and false only for ADVENTURE and "
                  "SPECTATOR. `--gamemode survival` does NOT open it.")

        # --- do the needles still detect anything? -------------------------
        #
        # Checked BEFORE the rows that depend on them. A "no gate here" row is
        # only worth its ink while the needles it is negative against still
        # match a real gate, and a needle that silently stopped matching would
        # make every such row read green forever.
        print("\n== gate spellings, each against a method known to contain it ==\n")
        for needle, (wcls, wmeth) in sorted(GATE_SPELLINGS.items()):
            if wcls not in cache:
                cache[wcls] = javap(td, wcls)
            wbody = method_body(cache[wcls], wmeth)
            ok = any(needle in t for _, t in wbody)
            if not ok:
                reds += 1
            print("  %-52s %-9s %s.%s"
                  % (needle[:52], "ok" if ok else "NEEDLE DEAD",
                     wcls.rsplit(".", 1)[-1], wmeth))

        # --- harness verbs at zero, and which kind of zero each is ---------
        print("\n== harness verbs exercised by nothing, and why ==\n")
        for verb, v in sorted(VERBS.items()):
            if v["cls"] not in cache:
                cache[v["cls"]] = javap(td, v["cls"])
            body = method_body(cache[v["cls"]], v["method"])
            if v["gated"]:
                present = any(v["gate_needle"] in t for _, t in body)
                verdict = "substrate" if present else "GATE GONE"
            else:
                # negative against EVERY spelling, not one string
                hit = [n for n in GATE_SPELLINGS
                       if any(n in t for _, t in body)]
                verdict = "coverage" if not hit else "GATE APPEARED"
                if hit:
                    v = dict(v, note=v["note"] + f"  [matched: {hit}]")
            if verdict in ("GATE GONE", "GATE APPEARED"):
                reds += 1
            print("  %-12s %-14s %s.%s" % (verb, verdict,
                  v["cls"].rsplit(".", 1)[-1], v["method"]))
            print("               %s" % v["note"])
        print("\n  A verb at zero reads the same either way in "
              "ci/corpus-coverage.py.\n  `substrate` needed a game mode; "
              "`coverage` needs a scenario, and is not this lane's.")

    if args.gates_only:
        print(f"\n{reds} red(s). This is a declared reading, not an enumeration — "
              "it never claims the gate set is closed.")
        return 1 if reds else 0

    print("\n== kinds this corpus has never carried ==\n")
    pats = args.recordings or DEFAULT_RECORDINGS
    recs = load_recordings(pats)
    if not recs:
        die(f"no recordings matched {pats} — refusing to print a zero with no denominator")
    kinds = decoded_kinds()
    cnt = collections.Counter()
    for f in recs.values():
        try:
            d = json.load(open(f))
        except (OSError, ValueError):
            continue
        for s in d.get("scenarios", []):
            for o in s.get("obs", []):
                cnt[o["kind"]] += 1
    n = len(recs)
    never = [k for k in kinds if cnt.get(k, 0) == 0]
    print(f"  N = {n} distinct vanilla recordings (deduped by sha256)")
    print(f"  {len(kinds)} kinds have a decoder arm; {len(never)} have never been recorded\n")
    print("  %-30s %-10s %-14s %s" % ("kind", "class", "lever", "why"))
    for k in never:
        why, lever, note = ATTRIBUTION.get(k, ("UNATTRIBUTED", None, "no reading — do not guess"))
        if why == "UNATTRIBUTED":
            reds += 1
        print("  %-30s %-10s %-14s %s"
              % (k, why, (lever.split("=")[1] if lever else "-"), note))

    # --- which CHECKS are carrying a dead declaration because of this ------
    #
    # A kind in a check's `observe` list that vanilla never emits contributes
    # nothing to that check's verdict: the check is decided by whatever else
    # it named. That is not automatically a defect — `ci/checks/gameplay/
    # README.md` REQUIRES `section_blocks_update` beside `block_update` as
    # insurance against a future the recordings have not reached. What is
    # worth reading is the narrower class: a declaration that is dead for a
    # reason no server can fix.
    print("\n== corpus checks declaring a substrate-dark kind ==\n")
    dark = {k for k in never
            if ATTRIBUTION.get(k, ("",))[0] in ("substrate", "both")}
    checks = sorted(glob.glob(os.path.join(ROOT, "ci", "checks", "gameplay", "*.toml")))
    rows = []
    for f in checks:
        try:
            src = open(f).read()
        except OSError:
            continue
        m = re.search(r"^observe\s*=\s*\[(.*?)\]", src, re.M | re.S)
        if not m:
            continue
        kinds = re.findall(r'"([a-z_]+)"', m.group(1))
        hit = [k for k in kinds if k in dark]
        if hit:
            rows.append((os.path.basename(f), len(hit), len(kinds), hit))
    if not checks:
        print("  no checks found — ci/checks/gameplay is missing from this tree")
    for name, nd, nt, hit in rows:
        print("  %-50s %d of %d declared: %s" % (name[:50], nd, nt, ", ".join(hit)))
    print(f"\n  {len(rows)} of {len(checks)} checks declare a kind that no recording "
          "has ever carried,\n  for substrate reasons.")
    print("  A dead declaration is NOT the same as a dead check: these are decided by "
          "the\n  kinds they named that DO arrive. Read them as narrower than their "
          "`observe` list\n  claims, never as passing on the strength of it.")

    unattributed = [k for k in never if k not in ATTRIBUTION]
    substrate = [k for k in never if ATTRIBUTION.get(k, ("",))[0] in ("substrate", "both")]
    print(f"\n  substrate-attributed: {len(substrate)} of {len(never)} never-recorded kinds")
    print("  A `substrate` row means VANILLA did not send it. It says nothing about "
          "whether this server would.\n  Cross-read with ci/unported-inventory.py --open "
          "before calling any of them a work item.")
    if unattributed:
        print(f"\n  UNATTRIBUTED: {unattributed} — a never-recorded kind with no reading "
              "is a red, not a finding.")
    return 1 if reds else 0


if __name__ == "__main__":
    sys.exit(main())
