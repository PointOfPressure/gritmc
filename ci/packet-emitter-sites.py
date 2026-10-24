#!/usr/bin/env python3
"""Where in the jar does vanilla CONSTRUCT each classified packet — and does it.

    ci/packet-emitter-sites.py                       # every trace::FAMILIES row
    ci/packet-emitter-sites.py --json out.json
    ci/packet-emitter-sites.py ClientboundPingPacket

# WHY THIS EXISTS: "WE DID NOT SEE IT" IS NOT "IT CANNOT BE SENT"

`trace::FAMILIES` tags two rows `no-emitter` — `ping` and `tab_list` — and the
claim is that vanilla never sends them, so a check on either could only ever be
vacuous. The census can only ever support that claim the weak way: it counts
zero over the scenarios it ran. Absence of evidence, over eighteen scenarios,
is exactly the shape of reasoning this project has been burned by — the
datapack grep that "proved" bearding inert because the marker is never in the
datapack in any version, and the harness that recorded silence from a player
who had never moved.

The strong form is a property of the jar: **count the `new` instructions**.
A packet class with no construction site anywhere under `net/` cannot be
emitted by any code path, whatever a scenario does or does not provoke.

Measured 2026-09-06 on the pinned 26.2 server jar, 7,445 classes under `net/`,
for every `trace::FAMILIES` row that names a packet class:

    ping                     0
    tab_list                 0
    chunk_batch_start        1   its OWN static initialiser
    entity_position          1   its OWN class (a static factory)
    keep_alive               1   ServerCommonPacketListenerImpl
    chunk_batch_finished     1   PlayerChunkSender
    forget_level_chunk       1   PlayerChunkSender
    level_chunk_with_light   1   PlayerChunkSender
    light_update             1   ChunkHolder
    set_chunk_cache_center   1   ChunkMap
    update_advancements      1   PlayerAdvancements
    set_time                 3   ServerClockManager x2, MinecraftServer
    player_info_update       5   PlayerList, ServerGamePacketListenerImpl x2,
                                 ServerPlayerGameMode, and its own class

**Two zeroes, and they are the two rows tagged `no-emitter`.** Nothing else in
the classification is zero, so the tag is not a label somebody could have put
anywhere.

# A SITE INSIDE THE PACKET'S OWN CLASS IS A FACTORY, NOT AN EMISSION

`chunk_batch_start` and `entity_position` each count 1, and in both cases the
site is the class itself — `ClientboundChunkBatchStartPacket.<clinit>` builds
the `INSTANCE` that `StreamCodec.unit` then sends by reference, and
`ClientboundEntityPositionSyncPacket` has a static factory that
`ServerEntity.sendChanges` calls. So a count of 1 does not prove a caller
exists any more than a count of 0 proves one does not, in general. What it
does prove is the direction that matters here: **0 means no code anywhere
under `net/` can build one**, and that is a fact about the jar rather than
about which eighteen scenarios happened to run.

# THE TRAP THIS SCRIPT WALKED INTO FIRST, WRITTEN DOWN

The first version scanned an extraction that was **11 classes short** — a
`[ -d net ] || unzip` guard skipped a re-extract over a directory an earlier,
narrower `unzip` had already created — and it reported **zero construction
sites for `ClientboundKeepAlivePacket`**, a packet this server receives every
fifteen seconds. A scan that cannot see a class reports it as unreferenced,
which is the flattering direction, so this script refuses on a class count
below `MIN_CLASSES` and always reads the jar directly rather than a directory
somebody else populated.

The reported quantity is therefore "construction sites", and the
*interpretation* lives in the family's row rather than in this script.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tempfile
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JAR = os.path.join(
    ROOT, "oracle-work", "vanilla-26.2", "versions", "26.2", "server-26.2.jar"
)
MIN_CLASSES = 7000

# `trace::FAMILIES` names kinds; the jar names classes. One table, here,
# because the mapping is not derivable from either side.
CLASS_OF_KIND = {
    "keep_alive": "common/ClientboundKeepAlivePacket",
    "ping": "common/ClientboundPingPacket",
    "set_time": "game/ClientboundSetTimePacket",
    "level_chunk_with_light": "game/ClientboundLevelChunkWithLightPacket",
    "chunk_batch_start": "game/ClientboundChunkBatchStartPacket",
    "chunk_batch_finished": "game/ClientboundChunkBatchFinishedPacket",
    "forget_level_chunk": "game/ClientboundForgetLevelChunkPacket",
    "light_update": "game/ClientboundLightUpdatePacket",
    "set_chunk_cache_center": "game/ClientboundSetChunkCacheCenterPacket",
    "player_info_update": "game/ClientboundPlayerInfoUpdatePacket",
    "tab_list": "game/ClientboundTabListPacket",
    "update_advancements": "game/ClientboundUpdateAdvancementsPacket",
    # The two kinds this recorder synthesises rather than receiving: both are
    # forms of a packet recorded under another kind, so they have no class of
    # their own and no construction site to count.
    "move_entity_heartbeat": None,
    "entity_position": "game/ClientboundEntityPositionSyncPacket",
}


def die(msg):
    print(f"packet-emitter-sites: {msg}", file=sys.stderr)
    sys.exit(2)


def main(argv):
    if not os.path.exists(JAR):
        die(f"{JAR} is not here — this needs the vanilla extract")
    with zipfile.ZipFile(JAR) as z:
        names = [n for n in z.namelist() if n.endswith(".class") and n.startswith("net/")]
        if len(names) < MIN_CLASSES:
            die(
                f"the jar lists only {len(names)} classes under net/, below the "
                f"{MIN_CLASSES} floor — a short scan reports every packet as "
                "unreferenced, which is the flattering direction"
            )
        # `--json <path>` takes a VALUE, and the first version of this line
        # read that value as a class-name filter, so every invocation that
        # wrote a file scanned nothing and wrote `{}`. An empty result is the
        # flattering direction again: zero sites for everything.
        want, skip = [], False
        for a in argv:
            if skip:
                skip = False
                continue
            if a == "--json":
                skip = True
            elif not a.startswith("--"):
                want.append(a)
        targets = {
            k: v
            for k, v in CLASS_OF_KIND.items()
            if v is not None and (not want or any(w in v for w in want))
        }
        tmp = tempfile.mkdtemp(prefix="emitter-sites-")
        z.extractall(tmp, members=names)

    out = {}
    for kind, path in sorted(targets.items()):
        base = path.split("/")[-1]
        pat = re.compile(r"new +#[0-9]+ +// class net/minecraft/network/protocol/[a-z]+/%s$" % base)
        sites = {}
        hits = subprocess.run(
            ["grep", "-rl", "--binary-files=text", base, os.path.join(tmp, "net")],
            capture_output=True,
            text=True,
        ).stdout.split()
        for c in hits:
            dis = subprocess.run(["javap", "-p", "-c", c], capture_output=True, text=True).stdout
            n = sum(1 for ln in dis.splitlines() if pat.search(ln.strip()))
            if n:
                sites[os.path.basename(c)[:-6]] = n
        out[kind] = {"class": path, "sites": sites, "total": sum(sites.values())}

    for kind, r in out.items():
        where = ", ".join(f"{k} x{v}" if v > 1 else k for k, v in sorted(r["sites"].items()))
        print(f"  {kind:24} {r['total']:3d}  {where}")

    if "--json" in argv:
        p = argv[argv.index("--json") + 1]
        with open(p, "w") as fh:
            json.dump(out, fh, indent=2, sort_keys=True)
        print(f"wrote {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
