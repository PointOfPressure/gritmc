#!/usr/bin/env python3
"""entity-broadcast-order.py — the order per-entity packets leave the server
in, vanilla against ours, for the entity sets this server can actually make.

# The finding, in one line

`ChunkMap.entityMap` is an `Int2ObjectOpenHashMap` — the plain one, not the
`Linked` one its neighbours use — and `ChunkMap.tick()` walks its `values()`
calling `TrackedEntity.updatePlayers` and `ServerEntity.sendChanges`. So the
order every per-entity broadcast goes out in within one tick is a hash of the
**entity id values**, and nothing else.

That makes this the reachable half of what §94 found in `Mob.serverAiStep`.
The goal phase `(tickCount + getId()) % 2` is behaviour-visible and this
server can never observe it, because it has no mobs. This is the same
dependency on the same numbers on a path that is live today: a player and a
dropped item are two entities, they are ids 1 and 2, and they both produce
packets in the same tick.

# It is a REQUIREMENT and not noise, which is unusual for this class

`Int2ObjectOpenHashMap`'s walk is a pure function of the key set. Measured
against the fastutil the pinned jar itself ships with, three cold JVMs agree,
and inserting the same ids in the reverse order gives the identical walk. So
vanilla agrees with itself about this forever — unlike the spawn yaw, unlike
`broadcastChangedChunks` — and matching it is a real parity requirement with
no distribution to argue about.

    ids {1,2}   vanilla walks  2 1     ours walks  2 1
    ids {1,2,3} vanilla walks  2 1 3   ours walks  2 1 3

# The divergence closed, and 21 rows went with it

Those two rows read `1 2` and `1 2 3` until roadmap §103. 21 of the 41 pinned
id sets differed; 0 do. `crates/rustmc-server/src/openhash.rs` reproduces the
ten inputs `ci/fastutil-open-hash-walk.py` reads out of the shipped fastutil,
and `Entities.live` is now that container.

**So this check had to change how it learns OUR column, not only what it
expects there.** It used to compute `sorted(set(ids))` and justify it from a
source read: the container was a `BTreeMap` and "a BTreeMap walks ascending"
is the language's contract. That justification is gone — the container is now
ours — and a check that reads our source and then states our walk would be
asserting the thing it is here to grade. It runs the walk instead, through
`entities::tests::dump_broadcast_walks`, and computes the divergence itself.

# What this check does and does not claim

It does NOT claim the divergence closing changed anything observable. Nothing
in the corpus can see a per-tick entity order: `ci/order-blind.py` files all
108 gameplay checks and finds 0 that can grade one, and
`ci/order-selfconsistency.py` returns UNMEASURABLE for this site. The evidence
for §103 is this golden and the unit tests behind it, and saying so is part of
the deliverable rather than a caveat on it.

What it gates is that both walks stay read rather than remembered — vanilla's
from the jar, ours by running it — so the day either side moves without the
pin moving with it, this goes red. Arm 5 of the falsifier now moves OURS AWAY
from vanilla's order and requires that red, which is the same arm it always
was, pointed the other way now that the two agree.

The golden walks are measured, not computed: reimplementing fastutil's hash
from memory is exactly the kind of plausible-but-wrong step this project does
not take. `--measure` runs `ci/probes/IterOrder.java` against the pinned
jar's own fastutil and rewrites the table.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
JAR_REL = "oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
FASTUTIL_REL = ("oracle-work/vanilla-26.2/libraries/it/unimi/dsi/fastutil/"
                "8.5.18/fastutil-8.5.18.jar")
PIN = os.path.join(HERE, "checks", "entity-broadcast-order.json")
PROBE = os.path.join(HERE, "probes", "IterOrder.java")
OURS = os.path.join(ROOT, "crates", "rustmc-server", "src", "entities.rs")
OURS_CONSUMER = os.path.join(ROOT, "crates", "rustmc-server", "src", "tracker.rs")

# The id sets to hold golden walks for. Chosen so the table covers what this
# server can reach today (one player; a player and an item; a player and two
# items) and enough beyond it that a probe returning the input unchanged
# would be caught by the rows where the walk is a real permutation.
ID_SETS = [
    [1],
    [1, 2],
    [1, 3],
    [2, 1],
    [1, 2, 3],
    [1, 2, 4],
    [1, 2, 3, 4],
    [1, 2, 3, 4, 5],
    [5, 4, 3, 2, 1],
    [1, 2, 3, 4, 5, 6, 7, 8],
    # And every id set that two or more entities are actually seen holding in
    # one window in the recordings on disk. Enumerated once and frozen here
    # rather than derived from the recordings at run time: a golden table that
    # regrows itself from whatever a tree happens to hold is a table nobody
    # can diff. `ci/order-selfconsistency.py` refuses if it meets a set this
    # list does not cover, which is how the list stays honest.
    [1, 4],
    [1, 5],
    [1, 6],
    [1, 7],
    [1, 8],
    [1, 8, 9],
    [1, 9],
    [1, 10],
    [1, 11],
    [1, 12],
    [1, 12, 13],
    [1, 15],
    [1, 15, 16],
    [2, 3],
    [2, 4],
    [2, 5],
    [2, 6],
    [2, 7],
    [2, 8],
    [2, 8, 9],
    [2, 9],
    [2, 10],
    [2, 11],
    [2, 12],
    [2, 14],
    [2, 15, 16],
    [2, 16],
    [12, 13],
    [13, 14],
    [14, 15],
    [15, 16],
]


def die(msg):
    print("entity-broadcast-order: REFUSED — %s" % msg, file=sys.stderr)
    sys.exit(3)


def jar_path():
    p = os.environ.get("RUSTMC_JAR") or os.path.join(ROOT, JAR_REL)
    return p if os.path.exists(p) else None


def javap(jar, cls):
    try:
        out = subprocess.run(["javap", "-p", "-c", "-constants", "-classpath", jar, cls],
                             capture_output=True, text=True, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        die("javap could not run (%s)" % e)
    if out.returncode != 0 or not out.stdout.strip():
        die("javap produced nothing for %s" % cls)
    return out.stdout


def read_vanilla_shape(jar):
    """The two bytecode facts the whole finding rests on."""
    cm = javap(jar, "net.minecraft.server.level.ChunkMap")
    lines = cm.splitlines()
    container = None
    for i, ln in enumerate(lines):
        if re.search(r"putfield\s+#\d+\s+// Field entityMap:", ln):
            for j in range(i - 1, max(-1, i - 20), -1):
                m = re.search(r"new\s+#\d+\s+// class ([a-zA-Z0-9/$]+)", lines[j])
                if m:
                    container = m.group(1)
                    break
            break
    if container is None:
        die("could not find ChunkMap.entityMap's construction — the reader is dead")

    # `tick()` must actually walk it, or the container's order reaches nothing.
    try:
        i = cm.index("  protected void tick();")
        body = cm[i:cm.index("\n\n", i)]
    except ValueError:
        die("ChunkMap.tick not found — the reader is dead")
    walks = ("Int2ObjectMap.values" in body
             and "ObjectCollection.iterator" in body)
    sends = "ServerEntity.sendChanges" in body
    if not walks or not sends:
        die("ChunkMap.tick no longer walks entityMap.values into "
            "ServerEntity.sendChanges (walks=%s sends=%s) — the finding's "
            "premise moved and the check must not answer from a stale pin"
            % (walks, sends))
    return {"entity_map_container": container.rsplit("/", 1)[-1],
            "tick_walks_values_into_sendChanges": True}


def read_ours():
    """Ours, from the source — the BINDING only, never the walk.

    This function used to end by stating that a `BTreeMap` walks ascending,
    and said so explicitly: "the container's contract, not an observation
    about this tree". That was sound while the container was the language's.
    §103 replaced it with `crate::openhash::IntOpenHashMap`, and "our own map
    walks fastutil's order" is a claim about code in this repository. Reading
    the source and then stating the walk would make this check assert the
    thing it exists to grade.

    So what is read here is only that the binding is intact — the container is
    the ported one, `Entities::all` walks it, and `Tracker::tick` calls that.
    The walk itself is measured by `measure_ours`, which runs it.
    """
    if not os.path.exists(OURS):
        die("our entity list source is missing: %s" % OURS)
    src = open(OURS, encoding="utf-8").read()
    if not re.search(r"by_id:\s*IntOpenHashMap<Entity>", src):
        die("our entity list's container was not found in %s — the Rust "
            "reader is dead" % OURS)
    m = re.search(r"pub fn all\(&self\) -> Vec<Entity> \{(.*?)\n    \}", src, re.S)
    if m is None:
        die("our Entities::all was not found — the Rust reader is dead")
    if ".values()" not in m.group(1):
        die("Entities::all no longer walks the map's values; this check "
            "cannot say what order it produces")
    # AND THE CONSUMER, for the same reason the vanilla side checks that
    # `ChunkMap.tick` actually walks its map. Grading `Entities::all`'s order
    # says nothing if the broadcast loop stopped calling it: the check would
    # keep comparing a function nothing reads, which is the failure mode this
    # whole branch is about.
    if not os.path.exists(OURS_CONSUMER):
        die("our broadcast loop's source is missing: %s" % OURS_CONSUMER)
    csrc = open(OURS_CONSUMER, encoding="utf-8").read()
    m = re.search(r"pub fn tick\(\s*&self,\s*entities: &Entities,(.*?)\n    \}",
                  csrc, re.S)
    if m is None:
        die("Tracker::tick was not found in %s — the consumer reader is dead"
            % OURS_CONSUMER)
    if "entities.all()" not in m.group(1):
        die("Tracker::tick no longer walks Entities::all, so this check would "
            "be grading the order of a function the broadcast loop does not call")
    # AND THE SECOND WALK. `ChunkMap.tick` walks `entityMap.values()` TWICE —
    # once for the main broadcast and once for the movedPlayers sweep — which
    # the pinned call sequence in `ci/checks/fastutil-open-hash-walk.json`
    # shows. Ours splits that map's two halves across `Entities.by_id` and
    # `Tracker.tracked`, so the sweep walks the second one, and a sweep left on
    # an ordered map would be a divergence this check could not see: it grades
    # the ids `Entities::all` returns and the sweep emits from a different
    # container.
    if not re.search(r"tracked:\s*Mutex<IntOpenHashMap<TrackedEntity>>", csrc):
        die("the tracker's own copy of entityMap is not the ported container, "
            "so ChunkMap.tick's second walk is in some other order")
    if "for_each_mut" not in m.group(1):
        die("Tracker::tick's movedPlayers sweep no longer walks the tracked "
            "map in its own order — ChunkMap.tick walks entityMap.values() "
            "twice and this check would be grading only the first")
    return {"container": "IntOpenHashMap<i32, Entity>",
            "walk": "measured by running Entities::all, not stated here",
            "consumer": "Tracker::tick over Entities::all"}


def measure_ours(id_sets):
    """OUR walk, by running our code.

    `cargo test -- --nocapture` over `entities::tests::dump_broadcast_walks`,
    which builds a real `Entities` for each pinned id set and prints what
    `all()` returns. That test asserts nothing, deliberately: if it asserted,
    a wrong walk would fail it, this would get an empty answer and refuse, and
    the check would never go red on the one condition it exists to catch.

    Every failure here is a refusal. A missing cargo, a build error, a set the
    dump did not answer — none of them is evidence about the order, and
    guessing `sorted(ids)` in their place is exactly how this check would come
    to report a divergence that no longer exists.
    """
    env = dict(os.environ)
    if not shutil.which("cargo", path=env.get("PATH", "")):
        cargo_bin = os.path.expanduser("~/.cargo/bin")
        env["PATH"] = cargo_bin + os.pathsep + env.get("PATH", "")
    if not shutil.which("cargo", path=env["PATH"]):
        die("cargo is not on PATH, so our walk cannot be run — and this check "
            "does not answer from a source read")
    try:
        r = subprocess.run(
            ["cargo", "test", "-q", "-p", "rustmc-server", "--lib",
             "entities::tests::dump_broadcast_walks", "--", "--exact", "--nocapture"],
            capture_output=True, text=True, timeout=1800, cwd=ROOT, env=env)
    except (OSError, subprocess.SubprocessError) as e:
        die("could not run our walk (%s)" % e)
    if r.returncode != 0:
        die("the walk dump did not run: %s"
            % ((r.stderr or r.stdout or "").strip()[-400:]))
    out = {}
    for line in r.stdout.splitlines():
        if not line.startswith("BCAST-WALK "):
            continue
        key, walk = line[len("BCAST-WALK "):].split("|")
        out[key] = [int(x) for x in walk.split()] if walk.strip() else []
    if not out:
        die("the walk dump printed nothing — it is not the reader this check "
            "thinks it is running")
    missing = [" ".join(str(i) for i in s) for s in id_sets
               if " ".join(str(i) for i in s) not in out]
    if missing:
        die("the walk dump answered %d sets and did not answer %d of them, "
            "starting with {%s}" % (len(out), len(missing), missing[0]))
    return out


def measure(jar_fastutil):
    """Run the probe against the pinned jar's own fastutil."""
    if not os.path.exists(jar_fastutil):
        die("no fastutil under %s" % jar_fastutil)
    if not os.path.exists(PROBE):
        die("the probe is missing: %s" % PROBE)
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        r = subprocess.run(["javac", "-cp", jar_fastutil, "-d", td, PROBE],
                           capture_output=True, text=True, timeout=300)
        if r.returncode != 0 or not os.path.exists(os.path.join(td, "IterOrder.class")):
            die("the probe did not compile: %s" % (r.stderr or "").strip()[:400])
        stdin = "\n".join(" ".join(str(i) for i in s) for s in ID_SETS) + "\n"
        r = subprocess.run(["java", "-cp", td + os.pathsep + jar_fastutil, "IterOrder"],
                           input=stdin, capture_output=True, text=True, timeout=300)
        if r.returncode != 0 or not r.stdout.strip():
            die("the probe produced nothing: %s" % (r.stderr or "").strip()[:400])
    table = {}
    for line in r.stdout.strip().splitlines():
        got, open_walk, link_walk = line.split("|")
        table[got] = {"open": [int(x) for x in open_walk.split()] if open_walk else [],
                      "linked": [int(x) for x in link_walk.split()] if link_walk else []}
    if len(table) != len(ID_SETS):
        die("the probe answered %d of %d id sets" % (len(table), len(ID_SETS)))
    return table


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as fh:
        for b in iter(lambda: fh.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--measure", action="store_true",
                    help="re-run the fastutil probe and rewrite the golden table")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()

    jar = jar_path()
    if jar is None:
        die("no pinned jar under %s" % os.path.join(ROOT, JAR_REL))

    vshape = read_vanilla_shape(jar)
    ours = read_ours()
    fu = os.path.join(ROOT, FASTUTIL_REL)

    if args.measure:
        table = measure(fu)
        pin = {"vanilla_shape": vshape, "ours": ours,
               "fastutil_sha256": sha256(fu),
               "fastutil_path": FASTUTIL_REL,
               "golden": table}
        # Written before the walk is measured, because the dump reads this
        # file for its id sets. On the first --measure of a new set list the
        # golden has to exist before ours can be asked about it.
        with open(PIN, "w", encoding="utf-8") as fh:
            json.dump(dict(pin, divergent_sets=[], ours_matches_vanilla=False),
                      fh, indent=2, sort_keys=True)
            fh.write("\n")
        mine = measure_ours(ID_SETS)
        pin["divergent_sets"] = sorted(
            k for k, v in table.items() if v["open"] != mine.get(k))
        pin["ours_matches_vanilla"] = not pin["divergent_sets"]
        with open(PIN, "w", encoding="utf-8") as fh:
            json.dump(pin, fh, indent=2, sort_keys=True)
            fh.write("\n")
        print("rewrote %s from a cold probe run" % PIN)
        return 0

    if not os.path.exists(PIN):
        die("no golden table at %s; run --measure once and commit it" % PIN)
    pin = json.load(open(PIN, encoding="utf-8"))

    # A pin measured against a different fastutil is not this jar's answer.
    if os.path.exists(fu) and pin.get("fastutil_sha256") != sha256(fu):
        die("the pinned golden was measured against a different fastutil "
            "(%s on disk) — re-measure rather than trusting it"
            % sha256(fu)[:16])

    mine = measure_ours(ID_SETS)
    rows = []
    for s in ID_SETS:
        key = " ".join(str(i) for i in s)
        g = pin["golden"].get(key)
        if g is None:
            die("the golden table has no row for {%s} — it was measured "
                "against a different id-set list" % key)
        rows.append((key, g["open"], g["linked"], mine[key]))

    divergent = [r for r in rows if r[1] != r[3]]

    if args.json:
        print(json.dumps({"rows": [{"ids": a, "vanilla_broadcast": b,
                                    "vanilla_tick_list": c, "ours": d}
                                   for a, b, c, d in rows],
                          "divergent": [r[0] for r in divergent]},
                         indent=2))
        return 0

    print("per-tick entity broadcast order — ChunkMap.tick over %s"
          % vshape["entity_map_container"])
    print("ours — Entities::all over %s, %s" % (ours["container"], ours["walk"]))
    print()
    print("  %-24s  %-24s  %-24s  %s"
          % ("ids, in insert order", "vanilla broadcast", "ours", ""))
    for key, vw, lw, ow in rows:
        print("  %-24s  %-24s  %-24s  %s"
              % ("{%s}" % key, " ".join(map(str, vw)), " ".join(map(str, ow)),
                 "DIFFERS" if vw != ow else ""))
    print()
    print("  the vanilla column ignores insert order — {1 2} and {2 1} walk the")
    print("  same — because an Int2ObjectOpenHashMap is keyed on the values.")
    print("  The linked map beside it, which is what EntityTickList uses, does")
    print("  not: {5 4 3 2 1} ticks as %s."
          % " ".join(map(str, pin["golden"]["5 4 3 2 1"]["linked"])))
    print()
    if divergent:
        print("%d of %d id sets differ. The smallest is {%s}, which is one "
              "player" % (len(divergent), len(rows), divergent[0][0]))
        print("and one dropped item — the two entity kinds this server has.")
    else:
        print("0 of %d id sets differ. 21 of them did before roadmap §103, "
              "including" % len(rows))
        print("  {1 2} — one player and one dropped item, the smallest set this")
        print("  server can build. Our column is not asserted from the source: it")
        print("  is what `Entities::all` returned when this check ran it.")
        print()
        print("  AND NO SCENARIO CAN SEE IT. ci/order-blind.py finds 0 of 108")
        print("  checks able to grade a per-tick entity order, and")
        print("  ci/order-selfconsistency.py returns UNMEASURABLE for this site.")
        print("  This row is a golden and a unit test, not a corpus verdict.")

    if args.check:
        now = {"vanilla_shape": vshape, "ours": ours,
               "divergent_sets": sorted(r[0] for r in divergent),
               "ours_matches_vanilla": not divergent}
        was = {k: pin[k] for k in now}
        if was != now:
            print("entity-broadcast-order: the derivation moved against its pin",
                  file=sys.stderr)
            for k in now:
                if was.get(k) != now[k]:
                    print("   %s: pinned %r, now %r" % (k, was.get(k), now[k]),
                          file=sys.stderr)
            return 1
        print("entity-broadcast-order: %d id sets, %d divergent, exactly as pinned"
              % (len(rows), len(divergent)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
