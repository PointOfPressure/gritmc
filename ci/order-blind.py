#!/usr/bin/env python3
"""order-blind.py — which gameplay checks could grade a per-tick entity order,
and the ratchet that fires the day one can.

# What this is for

`ci/entity-broadcast-order.py` established that our per-tick broadcast order
differed from vanilla's for 21 of 41 id sets, and `ci/order-selfconsistency.py`
established that no proxy in the corpus isolates it. Those two together were a
comfortable place to stop, and that was the danger: a divergence nobody can see
is a divergence nobody has to fix, and it stays unfixed until a scenario lands
that happens to see it — at which point it reads as a regression from whatever
that scenario changed.

# The divergence closed, so the ratchet turned round

Roadmap §103 ported the walk and the pin now records 0 divergent sets. **That
disarms the original test**: `can and divergence_open` can no longer fire,
because `divergence_open` is false, and an arm that cannot fire is a comment.

The rot the ratchet guards did not go away, it changed direction. Nothing in
the corpus can see this order — still 0 of 108 — so if our walk drifts back
off vanilla's, no scenario reddens. `ci/entity-broadcast-order.py --check`
catches a drift against its pin, but that pin has a `--measure` that rewrites
it, and a re-measure is how a closed divergence quietly reopens.

So this file now carries the closure as a FLOOR. The ledger records that the
order was closed; `--check` refuses if the broadcast pin ever says otherwise,
whatever a re-measure wrote there. Monotone, and pointed at the escape hatch
rather than at the scenario.

The original test is kept as well, because it is still the right answer if the
divergence ever is open: a check that can grade the order while ours differs
must not be green.

# The two halves, both derived

A check can grade the broadcast order only if BOTH hold:

  * its `observe` list names a kind that `ServerEntity.sendChanges` produces.
    The packet list is read from `ServerEntity`'s own bytecode, not typed in,
    so a packet added to that method joins the test by construction;
  * the recordings show that check's window naming two or more distinct
    entities on such a kind. One entity has no order.

A check with the first and not the second is BLIND-BY-POPULATION and may stop
being blind when a scenario's world changes under it. A check with neither is
BLIND-BY-PROJECTION and cannot start seeing it by accident.

# The population is COMMITTED, not found (gate-pipeline, 2026-09-30)

Until this date the second half read every `~/rustmc*/oracle-work/gameplay*/
vanilla.json` on the box at gate time. So the verdict for the tree under test
moved with whatever other lanes had recorded, under THEIR versions of
same-named scenarios: at 15:09 on 30 Sep `~/rustmc-train-c` recorded a corpus
in which `a-standing-player-is-rebroadcast-on-a-timer` shows two entities in
one tick, and every T0 on the box from then on read that check as
CAN-GRADE-ORDER — the damage branch's T0 went red on a recording neither it
nor main had made. `ci/order-selfconsistency.py` had the same defect and the
same repair.

`--check` and `--ledger` now read `ci/checks/order-blind-population.json`,
a tracked reduction of named recordings (per recording sha256, per scenario,
per kind: the most distinct entities in one `(kind, who, at_ms)` bucket — the
narrow reading below, applied at extract time). It changes only when someone
runs `--extract` and commits what entered. `--check` refuses `--recordings`.
`--verify-extract` re-derives the committed rows from whatever recordings
this box still holds; the ad-hoc mode (no flag) still measures the box.
"""

import argparse
import glob
import hashlib
import json
import os
import re
import subprocess
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
JAR_REL = "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
GAMEPLAY = os.path.join(HERE, "checks", "gameplay")
LEDGER = os.path.join(HERE, "checks", "order-blind.toml")
BCPIN = os.path.join(HERE, "checks", "entity-broadcast-order.json")
DEFAULT_GLOB = os.path.expanduser("~/rustmc*/oracle-work/gameplay*/vanilla.json")
CORPUS = os.path.join(HERE, "checks", "order-blind-population.json")
CORPUS_VERSION = 1

# `ServerEntity`'s packet class names, as the recorder's kind names. The map
# is declared; WHICH of these `ServerEntity` actually builds is derived. A
# packet class with no recorder kind is one the trace does not decode and is
# reported rather than dropped, because a kind this cannot name is a kind a
# check could observe and this could not see.
PACKET_TO_KIND = {
    "ClientboundMoveEntityPacket": ["move_entity", "move_entity_rot",
                                    "move_entity_heartbeat"],
    "ClientboundRotateHeadPacket": ["rotate_head"],
    "ClientboundSetEntityDataPacket": ["set_entity_data"],
    "ClientboundSetEntityMotionPacket": ["set_entity_motion"],
    "ClientboundUpdateAttributesPacket": ["update_attributes"],
    "ClientboundRemoveEntitiesPacket": ["remove_entities"],
    "ClientboundSetEquipmentPacket": ["set_equipment"],
    "ClientboundSetPassengersPacket": ["set_passengers"],
    "ClientboundSetEntityLinkPacket": ["set_entity_link"],
    "ClientboundMoveMinecartPacket": [],
    "ClientboundProjectilePowerPacket": [],
    "ClientboundBundlePacket": [],
}


def die(msg):
    print("order-blind: REFUSED — %s" % msg, file=sys.stderr)
    sys.exit(3)


def jar_path():
    p = os.environ.get("RUSTMC_JAR") or os.path.join(ROOT, JAR_REL)
    return p if os.path.exists(p) else None


def broadcast_kinds(jar):
    """The recorder kinds `ServerEntity.sendChanges` can put on the wire."""
    try:
        out = subprocess.run(["javap", "-p", "-c", "-constants", "-classpath", jar,
                              "net.minecraft.server.level.ServerEntity"],
                             capture_output=True, text=True, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        die("javap could not run (%s)" % e)
    if out.returncode != 0 or not out.stdout.strip():
        die("javap produced nothing for ServerEntity")
    classes = sorted(set(re.findall(
        r"// class net/minecraft/network/protocol/game/(Clientbound[A-Za-z]+)",
        out.stdout)))
    if not classes:
        die("no Clientbound packet found in ServerEntity — the packet reader "
            "is dead, and an empty kind set makes every check look blind")
    unknown = [c for c in classes if c not in PACKET_TO_KIND]
    if unknown:
        die("ServerEntity builds %s, which PACKET_TO_KIND does not name. Add "
            "it with its recorder kind rather than letting a broadcast packet "
            "go uncounted" % ", ".join(unknown))
    kinds = set()
    for c in classes:
        kinds.update(PACKET_TO_KIND[c])
    return classes, sorted(kinds)


def load_checks():
    if not os.path.isdir(GAMEPLAY):
        die("no gameplay checks under %s" % GAMEPLAY)
    checks = {}
    for f in sorted(os.listdir(GAMEPLAY)):
        if not f.endswith(".toml"):
            continue
        text = open(os.path.join(GAMEPLAY, f), encoding="utf-8").read()
        m = re.search(r'^name\s*=\s*"([^"]+)"', text, re.M)
        if m is None:
            continue
        obs = re.search(r"^observe\s*=\s*\[(.*?)\]", text, re.M | re.S)
        kinds = re.findall(r'"([^"]+)"', obs.group(1)) if obs else []
        checks[m.group(1)] = {"file": f, "observe": kinds}
    if not checks:
        die("no check declared a name under %s — the check reader is dead"
            % GAMEPLAY)
    return checks


def reduce_recording(d):
    """One recording -> {scenario: {kind: most distinct entities in one tick}}.

    WITHIN ONE TICK, not within the window. An order between two entities
    only exists where two entities emit in the same tick: two heartbeats
    three seconds apart are a sequence in time and say nothing about the
    walk. `(who, at_ms)` is the closest thing the recording has to a tick,
    and it is deliberately the narrow reading — the wide one (distinct
    entities anywhere in the window) counts a check as able to grade an
    order when its own file records those entities being bounded out of the
    projection by `compare::place`, which is how this tool first reported
    `a-standing-player-is-rebroadcast-on-a-timer` as gradeable.
    """
    out = {}
    for s in d["scenarios"]:
        byk = defaultdict(set)
        for o in s["obs"]:
            e = o.get("fields", {}).get("entity")
            if isinstance(e, dict) and "$entity" in e:
                byk[(o["kind"], o["who"], o["at_ms"])].add(e["$entity"])
        per = {}
        for (k, _who, _ms), es in byk.items():
            per[k] = max(per.get(k, 0), len(es))
        if per:
            out[s["name"]] = per
    return out


def scan(pattern, before=None):
    """Distinct recordings on this box: {sha256: (tree, reduced)}, file count.
    `before` (epoch seconds) keeps only files last written before it — how a
    population is re-declared as it stood at a named commit."""
    files = sorted(f for f in glob.glob(pattern)
                   if before is None or os.path.getmtime(f) <= before)
    if not files:
        die("no vanilla recordings matched %s" % pattern)
    seen = {}
    for f in files:
        try:
            d = json.load(open(f, encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if "scenarios" not in d:
            continue
        h = hashlib.sha256(json.dumps(d["scenarios"], sort_keys=True).encode()).hexdigest()
        if h not in seen:
            seen[h] = (f, reduce_recording(d))
    if not seen:
        die("%d files matched but none held a scenario block" % len(files))
    return seen, len(files)


def merge(reduced_list):
    pop = defaultdict(lambda: defaultdict(int))
    for red in reduced_list:
        for name, per in red.items():
            for k, n in per.items():
                pop[name][k] = max(pop[name][k], n)
    return pop


def entity_population(pattern):
    """AD-HOC ONLY: the box's recordings, as found. Never used by --check."""
    seen, nfiles = scan(pattern)
    return merge(r for _f, r in seen.values()), len(seen), nfiles


def load_corpus():
    """The committed population, or a refusal. Never globs."""
    if not os.path.exists(CORPUS):
        die("no committed population at %s. It is a TRACKED file, so its "
            "absence is a broken tree and not a fact about this box; rebuild "
            "it with --extract where the recordings are and commit it" % CORPUS)
    doc = json.load(open(CORPUS, encoding="utf-8"))
    if doc.get("version") != CORPUS_VERSION:
        die("%s is version %r and this checker reads %r"
            % (CORPUS, doc.get("version"), CORPUS_VERSION))
    recs = doc.get("recordings") or []
    if not recs:
        die("%s holds no recordings" % CORPUS)
    return doc


def corpus_population():
    doc = load_corpus()
    recs = doc["recordings"]
    return merge(r["pop"] for r in recs), len(recs), len(recs)


def write_extract(pattern, before=None):
    seen, nfiles = scan(pattern, before)
    recs = [{"sha256": h, "tree": f.split("/")[3] if f.startswith("/home/") else f,
             "pop": red} for h, (f, red) in sorted(seen.items())]
    doc = {
        "version": CORPUS_VERSION,
        "about": ("ci/order-blind.py's population: per distinct vanilla "
                  "recording, per scenario, per kind, the most distinct "
                  "entities in one (kind, who, at_ms) bucket. Read by --check "
                  "and --ledger INSTEAD of the box, so another lane's recording "
                  "cannot move this tree's verdict. Regenerate with --extract "
                  "and commit it in a reviewed change that says what entered; "
                  "never regenerated by a gate."),
        "source_glob": pattern,
        "recorded_before": before,
        "files_matched": nfiles,
        "recordings": recs,
    }
    with open(CORPUS, "w", encoding="utf-8") as fh:
        json.dump(doc, fh, sort_keys=True, indent=0, separators=(",", ":"))
        fh.write("\n")
    print("wrote %s — %d distinct recording(s) from %d file(s)"
          % (CORPUS, len(recs), nfiles))
    return 0


def verify_extract(pattern):
    """Re-derive each committed recording this box still holds; refuse on none."""
    doc = load_corpus()
    seen, _n = scan(pattern)
    checked = bad = 0
    for r in doc["recordings"]:
        got = seen.get(r["sha256"])
        if got is None:
            continue
        checked += 1
        if got[1] != r["pop"]:
            bad += 1
            print("order-blind --verify-extract: %s (%s) disagrees with the "
                  "recording it names" % (r["sha256"][:12], r["tree"]), file=sys.stderr)
    if checked == 0:
        die("none of the %d committed recordings is on this box; nothing verified"
            % len(doc["recordings"]))
    print("order-blind --verify-extract: %d of %d committed recordings re-derived, "
          "%d disagree" % (checked, len(doc["recordings"]), bad))
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--recordings", default=None,
                    help="ad-hoc glob (default: every tree on the box); "
                         "refused with --check and --ledger")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--ledger", action="store_true", help="rewrite the ledger")
    ap.add_argument("--extract", action="store_true",
                    help="re-declare the committed population from the box")
    ap.add_argument("--verify-extract", action="store_true",
                    help="re-derive the committed population from the recordings "
                         "this box still holds")
    ap.add_argument("--recorded-before", type=float, default=None,
                    help="with --extract: epoch seconds; only files written before it")
    args = ap.parse_args()
    pattern = args.recordings or DEFAULT_GLOB
    if args.extract:
        return write_extract(pattern, args.recorded_before)
    if args.verify_extract:
        return verify_extract(pattern)
    if (args.check or args.ledger) and args.recordings:
        die("--check and --ledger read the committed population (%s) and "
            "cannot be given --recordings: a gate pointed at a glob measures "
            "whichever lanes recorded last" % os.path.basename(CORPUS))

    jar = jar_path()
    if jar is None:
        die("no pinned jar under %s" % os.path.join(ROOT, JAR_REL))
    classes, kinds = broadcast_kinds(jar)
    checks = load_checks()
    if args.check or args.ledger:
        pop, N, nfiles = corpus_population()
    else:
        pop, N, nfiles = entity_population(pattern)

    rows = []
    for name, c in sorted(checks.items()):
        watched = sorted(set(c["observe"]) & set(kinds))
        if not watched:
            rows.append({"name": name, "file": c["file"], "status": "BLIND-BY-PROJECTION",
                         "watched": [], "entities_in_one_tick": 0})
            continue
        mx = max((pop.get(name, {}).get(k, 0) for k in watched), default=0)
        rows.append({"name": name, "file": c["file"],
                     "status": "CAN-GRADE-ORDER" if mx >= 2 else "BLIND-BY-POPULATION",
                     "watched": watched, "entities_in_one_tick": mx})

    can = [r for r in rows if r["status"] == "CAN-GRADE-ORDER"]
    bypop = [r for r in rows if r["status"] == "BLIND-BY-POPULATION"]

    if args.ledger:
        with open(LEDGER, "w", encoding="utf-8") as fh:
            fh.write("# ci/checks/order-blind.toml — what each gameplay check can see\n"
                     "# about ENTITY ORDER. Written by `ci/order-blind.py --ledger`.\n"
                     "#\n"
                     "# `--check` refuses the moment a check becomes CAN-GRADE-ORDER\n"
                     "# while ci/checks/entity-broadcast-order.json still records our\n"
                     "# order as differing from vanilla's. That is the whole point: a\n"
                     "# divergence nobody can see is one nobody has to fix, and it\n"
                     "# stays unfixed until a scenario lands that happens to see it,\n"
                     "# at which point it reads as a regression from that scenario.\n"
                     "#\n"
                     "# AND SINCE ROADMAP §103 CLOSED IT, `--check` also refuses if\n"
                     "# that pin ever records the order as differing again. The pin\n"
                     "# has a `--measure` that rewrites it, and with 0 of 108 checks\n"
                     "# able to grade this order, a re-measure is the only way a\n"
                     "# closed divergence could quietly reopen. `broadcast_order_closed`\n"
                     "# below is that floor. Removing the line disarms the refusal,\n"
                     "# which is deliberate — it must be a visible edit and not a\n"
                     "# side effect of running a tool.\n"
                     "#\n"
                     "# BLIND-BY-PROJECTION   observes no kind ServerEntity emits.\n"
                     "#                       Cannot start seeing order by accident.\n"
                     "# BLIND-BY-POPULATION   observes such a kind, but only ever one\n"
                     "#                       entity emits it. May stop being blind\n"
                     "#                       when the scenario's world changes.\n"
                     "# CAN-GRADE-ORDER       two or more entities, one order-bearing\n"
                     "#                       kind. Nothing is here yet.\n"
                     "#\n"
                     "# `entities_in_one_tick` is an OBSERVATION and not a gate\n"
                     "# floor, which is why it is not named `max_*`: the gate\n"
                     "# reads `status`, and a `max_` name here would register as\n"
                     "# a merge-blocking floor that nothing enforces.\n"
                     "#\n")
            bc = json.load(open(BCPIN, encoding="utf-8")) if os.path.exists(BCPIN) else {}
            fh.write("broadcast_order_closed = %s\n"
                     % ("true" if bc.get("ours_matches_vanilla") else "false"))
            fh.write("N_distinct_recordings = %d\nfiles = %d\n" % (N, nfiles))
            fh.write("serverentity_packets = %s\n" % json.dumps(classes))
            fh.write("order_bearing_kinds = %s\n\n" % json.dumps(kinds))
            for r in rows:
                if r["status"] == "BLIND-BY-PROJECTION":
                    continue
                fh.write("[[check]]\nname = %s\nfile = %s\nstatus = %s\n"
                         "watched = %s\n"
                         "# NOT a floor: the count of distinct entities this check\n"
                         "# has been seen to observe on one order-bearing kind in\n"
                         "# ONE TICK. The gate reads `status`, never this.\n"
                         "entities_in_one_tick = %d\n\n"
                         % (json.dumps(r["name"]), json.dumps(r["file"]),
                            json.dumps(r["status"]), json.dumps(r["watched"]),
                            r["entities_in_one_tick"]))
            fh.write("[summary]\nchecks = %d\nblind_by_projection = %d\n"
                     "blind_by_population = %d\ncan_grade_order = %d\n"
                     % (len(rows),
                        sum(1 for r in rows if r["status"] == "BLIND-BY-PROJECTION"),
                        len(bypop), len(can)))
        print("rewrote %s" % LEDGER)
        return 0

    print("what the corpus can see about entity order — N = %d distinct "
          "recordings (%d files)" % (N, nfiles))
    print("  ServerEntity builds %d packet classes, %d order-bearing recorder "
          "kinds" % (len(classes), len(kinds)))
    print()
    print("  %-20s %d" % ("BLIND-BY-PROJECTION",
                          sum(1 for r in rows if r["status"] == "BLIND-BY-PROJECTION")))
    print("  %-20s %d" % ("BLIND-BY-POPULATION", len(bypop)))
    print("  %-20s %d" % ("CAN-GRADE-ORDER", len(can)))
    print()
    for r in bypop[:12]:
        print("    by population: %-44s %s max=%d"
              % (r["name"][:44], ",".join(r["watched"])[:34], r["entities_in_one_tick"]))
    for r in can:
        print("    CAN GRADE: %-40s %s max=%d"
              % (r["name"][:40], ",".join(r["watched"]), r["entities_in_one_tick"]))

    if args.check:
        if not os.path.exists(LEDGER):
            die("no ledger at %s; run --ledger once and commit it" % LEDGER)
        if not os.path.exists(BCPIN):
            die("no broadcast-order pin at %s" % BCPIN)
        divergence_open = not json.load(open(BCPIN, encoding="utf-8"))["ours_matches_vanilla"]
        ledger_text = open(LEDGER, encoding="utf-8").read()
        was_closed = "broadcast_order_closed = true" in ledger_text
        if was_closed and divergence_open:
            print("order-blind: the per-tick broadcast order was closed (roadmap "
                  "§103, 0 of 41 id sets divergent) and "
                  "ci/checks/entity-broadcast-order.json now records ours as "
                  "differing from vanilla's again. NOTHING IN THE CORPUS CAN SEE "
                  "THAT — 0 of 108 checks can grade a per-tick entity order — "
                  "which is the entire reason this refusal exists rather than a "
                  "scenario going red. Restore the walk, or take the closure out "
                  "of the ledger and say in the roadmap what reopened it.",
                  file=sys.stderr)
            return 1
        if can and divergence_open:
            print("order-blind: %d check(s) can now grade a per-tick entity "
                  "order while ci/checks/entity-broadcast-order.json still "
                  "records ours as differing from vanilla's: %s. Close the "
                  "divergence or say in the ledger why the check does not "
                  "see it — it must not stay green either way."
                  % (len(can), ", ".join(r["name"] for r in can)), file=sys.stderr)
            return 1
        for r in bypop + can:
            if json.dumps(r["name"]) not in ledger_text:
                die("check %r observes an order-bearing kind and has no ledger "
                    "entry" % r["name"])
        print("order-blind: %d checks, %d blind by projection, %d by "
              "population, %d can grade order"
              % (len(rows),
                 sum(1 for r in rows if r["status"] == "BLIND-BY-PROJECTION"),
                 len(bypop), len(can)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
