#!/usr/bin/env python3
"""Which positive control does each gameplay check need, and where to arm it.

# What this is for

`ci/corpus-falsifiable.py` breaks a **recording** and asks whether the
comparator noticed; this picks a subsystem to break in the **server** and asks
whether the check noticed. The difference is the whole point: a mutated byte
moves the verdict of very nearly anything, because no implementation had to
produce it. A wrong implementation is a difference the check was written to
catch, or else it was not.

The controls live in `crates/rustmc-server/src/sabotage.rs`, are armed by
workspace, and one corpus run carries as many of them as there are scenarios.
This script writes the `RUSTMC_SABOTAGE` string that arms them.

# A broad control set per workspace, and why breadth beats attribution

The first draft armed exactly one control per workspace so that a red could be
attributed to a subsystem. That is the wrong trade, twice over.

**Attribution is the secondary question and it is recoverable anyway.** The
question is whether a real difference can redden the check at all; which
subsystem produced the difference matters only afterwards, and the firing
counters answer it — an armed control that never fired cannot be the reason
for anything.

**And the mechanisms are not separable from a recording.** A door's upper half,
a fence connecting to its neighbour and a lamp beside a lever are all "a block
update at a position the step did not touch". The first is one `setBlock`
inside `setPlacedBy`, the second is `updateShape` off a neighbour update, the
third is power crossing between blocks. Choosing one control per scenario means
guessing which, and a guess that lands wrong produces a green that means
nothing at all — the check survived a control that could never have moved it.

So every workspace is armed with all four **consequence** controls, which are
the ones no scenario's *setup* depends on:

    sched        nothing ever happens on a later tick than its cause
    neighbors    `updateNeighborsAt` visits nobody
    structure    the second block of a multi-block is never written
    blockentity  the block entity is never described

plus exactly one **input** control, chosen by what the measured step does,
because these four do break a setup that used them:

    click        the steps include a container verb
    menu         the projection contains `open_screen`
    use          the measured step right-clicks a block that is already there —
                 the position it would place into stays air
    placement    everything else: the claim is what `getStateForPlacement`
                 computed

A check that survives that server is a check that cannot go red. A check that
reddens has its positive control, and the counters say which control fired.
"""


from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path

FACE = {"up": (0, 1, 0), "down": (0, -1, 0), "north": (0, 0, -1),
        "south": (0, 0, 1), "west": (-1, 0, 0), "east": (1, 0, 0)}

CONTAINER_VERBS = {"container_click", "close_container", "drop", "swap_offhand"}

# Armed in every workspace. None of these is reachable from a scenario's setup
# in a way that changes what the setup leaves behind — see the module note.
ALWAYS = ("sched", "neighbors", "structure", "blockentity")

# The scheduled-tick checks, by scenario name.
#
# Named rather than derived because the projection blanks `when`: a
# `block_update` that arrived four ticks after its cause and one that arrived
# in the same tick are the same row in a recording, so nothing in the data
# distinguishes them. The list is short and every entry cites the vanilla rule
# whose delay it measures.
SCHEDULED = {
    "button-unpresses-itself",                        # ButtonBlock.tick
    "a-wooden-button-stays-pressed-longer",           # ButtonBlock.ticksToStayPressed
    "a-redstone-torch-turns-off-when-its-block-is-powered",  # RedstoneTorchBlock.tick
    "a-repeater-delays-the-signal-it-passes-on",      # DiodeBlock.tick
    "an-observer-pulses-at-the-block-it-faces",       # ObserverBlock.tick
}


def offset(pos, face):
    f = FACE[face]
    return (pos[0] + f[0], pos[1] + f[1], pos[2] + f[2])


def occupancy(setup):
    """Where the setup left a block. `place on=X face=F` writes at X+F."""
    out = set()
    for a in setup:
        if a.get("do") == "place":
            out.add(offset(a["on"], a.get("face", "up")))
    return out


def touched(steps):
    out = set()
    for s in steps:
        d = s.get("do")
        if d == "place":
            out.add(tuple(s["on"]))
            out.add(offset(s["on"], s.get("face", "up")))
        elif d in ("dig", "finish_dig"):
            out.add(tuple(s["at"]))
    return out


def block_of(state):
    # `state` is a block-state string on a `block_update` and an integer menu
    # counter on `container_set_slot`. Same key, two types, one of them not a
    # block at all.
    """A state's block id — everything before the `[`.

    Item ids and block ids coincide for everything the corpus places; the
    handful that do not (`minecraft:redstone` places `redstone_wire`) are held
    by no scenario whose step is a placement, and a mismatch here reads as
    `use`, which is the conservative answer.
    """
    return state.split("[", 1)[0] if isinstance(state, str) else ""


# Item ids whose block is not spelled the same way. Short by design: an entry
# here is a claim about the registry, and a wrong one silently reclassifies a
# check's control.
ITEM_BLOCK = {"minecraft:redstone": "minecraft:redstone_wire"}


def family_of(state_or_item):
    """The last underscore-separated token of a block or item id.

    `oak_sign` and `oak_wall_sign` share `sign`; `redstone_torch` and
    `redstone_wall_torch` share `torch`. A hoe and farmland share nothing,
    which is the answer that matters.
    """
    name = ITEM_BLOCK.get(block_of(state_or_item), block_of(state_or_item))
    # **After the namespace, not before it.** `minecraft:torch` has no
    # underscore in its path and one in nothing else, so splitting the whole id
    # returned `minecraft:torch` against `wall_torch`'s `torch` and filed
    # `150-torch-on-a-wall-faces-out` — a placement rule — as a right-click.
    path = name.split(":", 1)[-1]
    return path.rsplit("_", 1)[-1] if path else ""


def positions(obs):
    """Every position a projected observation names, `section_blocks_update`
    unpacked — its payload is a list and the outer `at` is only the section."""
    out = []
    for o in obs:
        f = o.get("fields") or {}
        if isinstance(f.get("blocks"), list):
            out.extend(tuple(b["at"]) for b in f["blocks"])
        elif isinstance(f.get("at"), list):
            out.append(tuple(f["at"]))
    return out


def block_families(projected):
    """Every block family the corpus records anywhere.

    Used to tell an item that places something from one that does not, which
    no data file in this tree answers directly: `minecraft:stick` places
    nothing and `minecraft:oak_log` places a log, and the difference decides
    whether a `place` step is a placement or a right-click when the check's own
    projection carries no block state to read.
    """
    out = set()
    for s in projected.values():
        for o in (s.get("declared") or []):
            f = o.get("fields") or {}
            for st in ([f.get("state")] + [b.get("state") for b in (f.get("blocks") or [])]):
                if isinstance(st, str) and st:
                    out.add(family_of(st))
    return out


def input_control(doc, obs, families=frozenset()):
    """The one control whose subsystem the measured step drives.

    `use` versus `placement` is decided by the recording, not by the setup: a
    `place` step whose target position — `on` plus `face` — stays air while
    the clicked block itself changes state is a right-click that ran
    `useWithoutItem`, and one that leaves a block there is a placement. Asking
    instead whether the setup had put something at `on` answers yes for nearly
    every scenario in the corpus, because a block is always placed *against*
    something.
    """
    steps = doc.get("step") or []
    if {s.get("do") for s in steps} & CONTAINER_VERBS:
        return "click", "the steps include a container verb"
    kinds = {o.get("kind") for o in obs}
    if "open_screen" in kinds:
        return "menu", "the projection contains open_screen"
    # **Only observations that carry a state may write into this map.**
    # `block_entity_data` names a position and has no `state`, and it arrives
    # after the `block_update` that put the block there — so recording it as
    # the empty string erased the placed block for all eighteen checks in the
    # 76x family and filed every one of them as a right-click.
    seen = {}
    for o in obs:
        f = o.get("fields") or {}
        if isinstance(f.get("blocks"), list):
            for b in f["blocks"]:
                if isinstance(b.get("state"), str) and b["state"]:
                    seen[tuple(b["at"])] = b["state"]
        elif isinstance(f.get("at"), list) and all(isinstance(v, int) for v in f["at"]):
            if isinstance(f.get("state"), str) and f["state"]:
                seen[tuple(f["at"])] = f["state"]
    # **The setup is walked for what it leaves in the player's hand and for
    # nothing else.** An earlier draft decided on the first `place` it saw,
    # which is always a setup one — the rig's own ground block — and so called
    # every right-click check a placement.
    held = None
    for a in (doc.get("setup") or []):
        if a.get("do") == "hold":
            held = a.get("item")
    for a in steps:
        if a.get("do") == "hold":
            held = a.get("item")
        if a.get("do") != "place":
            continue
        into = offset(a["on"], a.get("face", "up"))
        # **Against the held item, not against emptiness.** A block clicked
        # onto a *replaceable* one lands at the clicked position rather than at
        # `on + face` — snow stacking and slab merging both do — and reading
        # "the target stayed air, so this was a right-click" calls those uses.
        # The held item settles it: if what appeared is what the player was
        # holding, the step placed something.
        for at in (into, tuple(a["on"])):
            if at in seen and held and block_of(seen[at]) == block_of(held):
                return "placement", "the step leaves the held block at " + str(at)
        # **And where the two ids differ, the family name decides.**
        # A sign item places `oak_wall_sign` and a redstone torch item places
        # `redstone_wall_torch`, so an exact id comparison files those as
        # right-clicks. The last underscore-separated token is the block
        # family and it survives the `_wall_` infix.
        #
        # An earlier draft used the setup's occupancy for this — "a block
        # appeared where the setup left nothing" — and it is wrong twice: a
        # door's upper half is written by `setPlacedBy` at a position no setup
        # `place` names, and a hoe turns world terrain into farmland at a
        # position the setup never touched either. Both are right-clicks and
        # both read as placements.
        for at in (into, tuple(a["on"])):
            if at in seen and held and family_of(seen[at]) == family_of(held):
                return "placement", f"the held item's block family appeared at {at}"
        if tuple(a["on"]) in seen and not seen[tuple(a["on"])].startswith("minecraft:air"):
            return "use", "the clicked block changed and the held item is not what appeared"
        # **And where the projection carries no block state at all, the setup
        # decides.** `490-a-bystander-sees-a-chest-open` observes from the
        # bystander and its whole projection is one `block_event`, which names
        # a position and no state. Clicking a position the setup already filled,
        # with nothing in the recording saying a block appeared, is a
        # right-click.
        # **Only when the held item places nothing.** The first draft of this
        # fallback took the occupancy alone, and a check that places a real
        # block while observing only `block_changed_ack` — which is the whole
        # shape of a vacuous check, and is the fixture
        # `ci/corpus-grades-falsify.sh` builds — clicks a filled position too.
        # Its own projection cannot say a block appeared; the rest of the
        # corpus can, because the family is recorded somewhere.
        if (not seen and tuple(a["on"]) in occupancy(doc.get("setup") or [])
                and held and family_of(held) not in families):
            return "use", "the step clicks a filled position holding an item that places nothing"
    return "placement", "the claim is the computed placement state"


def main(argv):
    if len(argv) != 3:
        print("usage: corpus-controls.py <checks-dir> <projection.json>", file=sys.stderr)
        return 2
    checks, proj = Path(argv[1]), Path(argv[2])
    projected = {s["name"]: s for s in json.load(open(proj))["scenarios"]}
    # Workspace index is the scenario's index in `scenario::load_dir`, which
    # sorts the directory's paths. Same sort here or every control is armed
    # one workspace away from the check it was chosen for.
    paths = sorted(p for p in checks.glob("*.toml"))
    families = block_families(projected)
    rows, by_control = [], {}
    for i, p in enumerate(paths):
        doc = tomllib.load(open(p, "rb"))
        obs = (projected.get(doc["name"]) or {}).get("declared") or []
        kind, why = input_control(doc, obs, families)
        t = touched(doc.get("step") or [])
        away = [q for q in positions(obs)
                if all(isinstance(v, int) for v in q) and q not in t]
        rows.append({"ws": i, "file": p.name, "name": doc["name"],
                     "input_control": kind, "why": why,
                     "controls": sorted((kind, *ALWAYS)),
                     "away": len(away), "projected": len(obs)})
        for k in (kind, *ALWAYS):
            by_control.setdefault(k, []).append(i)
    # `*` where a control covers every workspace. Not cosmetic: the explicit
    # form of the four `ALWAYS` controls is 108 numbers each, and a
    # `RUSTMC_SABOTAGE` nobody can read is a `RUSTMC_SABOTAGE` nobody checks.
    every = set(range(len(paths)))
    spec = ";".join(
        f"{k}:*" if set(v) == every else f"{k}:{','.join(map(str, sorted(v)))}"
        for k, v in sorted(by_control.items()))
    print(json.dumps({"spec": spec, "rows": rows}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
