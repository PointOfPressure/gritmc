#!/usr/bin/env python3
"""What the `instanceof ServerLevel` gate hides, per probe rule, from the jar.

# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/server-level-gates.py              the gates and what each one hides
#     ci/server-level-gates.py --check      refuse if a gate, a population or a
#                                           hidden effect moved
#     ci/server-level-gates.py --emit       write data/vanilla/server_level_gates.json
#                                           for the dumpers to stamp rows from
#     ci/server-level-gates.py --table      markdown, for a cold start
#
# WHAT BREAKS IF YOU IGNORE THIS: a rule whose entire body sits behind
# `if (level instanceof ServerLevel)` returns a plausible value in a harness
# that is not one, the probe records that value as a fact about the block, and
# the row reads `consumes: true, writes: []` — which for `minecraft:chest` is
# right by coincidence and for `minecraft:sweet_berry_bush` is **false**.
# ===== end interface; everything below is why =====

# # A REFUSAL IS LOUD, AN ABSENCE IS NOT, AND THIS IS THE THIRD THING

`ci/refusal-ledger.py` counts what the generators said they could not answer.
`ci/probe-absences.py` counts what they never said anything about. This counts
what they said **confidently and without running the rule** — a row that exists,
carries cells, and was decided by what the harness *is* rather than by what the
block does.

`ci/probe-absences.py` found the population and reported it deliberately rather
than refusing it: six of the eight rules have rows, and refusing the gate
deletes every chest, barrel and shulker box row in the table over a branch this
project may never take. That was right. But "reported" meant reported *beside*
the table, in a tool's stdout, and the rows themselves still said nothing. This
puts it **in the row**, which is where a consumer reads.

# # WHY THE HARNESS CANNOT SEE THIS AND THE BYTECODE CAN

`instanceof` is answered by the JVM against the object's class. A `Level`
subclass cannot observe that it was asked, cannot refuse, and cannot log. There
is no hook. `SignBlock` is the sole exception in the whole registry and only
because *vanilla itself* announces the branch:

    if (!(level instanceof ServerLevel serverLevel)) {
        Util.pauseInIde(new IllegalStateException("Expected to only call this on server"));
        return InteractionResult.CONSUME;
    }

`Util.pauseInIde` is reachable — `SharedConstants.IS_RUNNING_IN_IDE` is a
non-final `public static boolean` beside a public `Util.setPause` — so
`UseGolden` refuses on it, and that is the only reason the 1,344 sign cells are
refusals instead of rows. The other seven return in silence. So the fact has to
come from **outside** the run, from the bytecode, which is what this does.

# # WHAT `hides` IS, AND WHY IT IS A CFG WALK AND NOT AN OFFSET RANGE

`hides` is the set of effects the rule performs on the far side of the gate,
mapped into a closed vocabulary. Deriving it by taking the instructions between
the `instanceof` and its branch target is wrong, and `SweetBerryBushBlock` is
the counter-example that proves it: its body is

    if (age > 1) {                       // 13: if_icmple 130
        if (level instanceof ServerLevel serverLevel) {   // 18: ifeq 126
            ...drop, sound, setBlock, gameEvent...
        }
        return SUCCESS;                  // 126
    }
    return super.useWithoutItem(...);    // 130  <- NOT gated, higher offset

An offset range `[gate, target)` misses nothing here, but a range `[gate, end)`
picks up the `super` call at 130, which the gate has nothing to do with. And the
inverted idiom — `ifeq FALSE; ...; goto AFTER; FALSE: return X; AFTER: ...` —
puts the gated code *after* the branch target, so no single range works for both.
`RespawnAnchorBlock` and `SignBlock` are that idiom; `BarrelBlock`, `ChestBlock`,
`ShulkerBoxBlock`, `EnderChestBlock` and `SweetBerryBushBlock` are the other.

So the derivation is a control-flow walk:

  * `taken`     = instructions reachable from the `ifeq`'s **fall-through**,
                  which is the branch where the level *is* a `ServerLevel`;
  * `not_taken` = instructions reachable from the `ifeq`'s **target**, which is
                  what this harness actually ran;
  * `hides`     = invocations in `taken` minus invocations in `not_taken`.

The subtraction is what makes the join correct without special-casing it. Code
after the two branches rejoin — `126: getstatic SUCCESS; areturn` — is reachable
from both and cancels, which is right: the harness did reach it. The `age <= 1`
path is reachable from neither and never appears.

# # THE VOCABULARY IS CLOSED AND REFUSES WHAT IT DOES NOT KNOW

Every invocation in `hides` must map to a token in `EFFECTS`. An unmapped
invocation is an error, not a shrug — `UNMAPPED` in `--check` — because the
whole value of the column is that `block-state` means "this gate hides a write
to the world" and a vocabulary that silently drops what it has not seen before
cannot mean that. A 26.3 jar that adds an effect behind one of these gates
fails here and gets a token, rather than quietly widening a category.

# # THE GUARD, AND WHAT IS ACTUALLY CHECKED ABOUT IT

Which *cells* reach the gate is a separate question from what the gate hides,
and it is not answered here: it needs the real `BlockState`, which only the
dumper holds. The ledger declares it as a state predicate — `always`,
`age>1`, `charges>0` — `--emit` hands it to the dumper, and the dumper
evaluates it against the property and stamps the cell.

What *is* checked here is the half the bytecode can settle: whether a guard
exists at all. If no conditional branch precedes the gate the ledger must say
`always`, and if one does it must not. That arm is why a vanilla update adding
a condition above one of these gates cannot leave a stale `always` in place,
silently widening a stamped population to cells that never reach the branch.
"""
import json
import os
import pathlib
import re
import subprocess
import sys
import tomllib
import zipfile

ROOT = pathlib.Path(os.environ.get("SERVER_LEVEL_GATES_ROOT")
                    or os.environ.get("PROBE_ABSENCES_ROOT")
                    or pathlib.Path(__file__).resolve().parent.parent)
DATA = ROOT / "data" / "vanilla"
LEDGER = ROOT / "ci" / "checks" / "server-level-gates.toml"
EMIT = DATA / "server_level_gates.json"
PKG = "net/minecraft/world/level/block/"
SERVER_LEVEL = "net/minecraft/server/level/ServerLevel"
PAUSE = "pauseInIde"

# The probe methods, and the table each one feeds. Same two as
# `ci/probe-absences.py` and for the same reason: these are the harnesses that
# **answer** rather than throw, so a gate they fail produces a row.
PROBES = {
    "setPlacedBy": "set_placed_by.json",
    "useWithoutItem": "use_without_item.json",
}

# # THE FIFTH POPULATION, AND WHY IT IS SWEPT BUT NOT REQUIRED
#
# `use_item_on.json` is the other half of the right-click and nothing under
# `crates/` opens it, so most trees never build it. Making it a plain member of
# `PROBES` turns `--check` red in every one of those, which is why the branch
# that measured this population left it in `--census` and asserted nothing.
#
# It is swept now, and the exemption is exactly one thing: **a tree without the
# table skips this probe and says so**, where a tree without either swept
# table still refuses. Everything else is identical — ledger entries, guard,
# columns, the stamp cross-check. So CI, which generates all three, gets the
# full arm set, and a laptop that has never run the dumper is not lied to
# about what was checked.
OPTIONAL_PROBES = {"useItemOn": "use_item_on.json"}


def probes():
    """The swept probes for this tree: the two required, plus any optional
    one whose table is actually here -- and, under `--emit`, every optional
    one regardless.

    `--emit` is the INPUT to the optional table's own dumper:
    `ci/use-item-on-golden.sh` runs it and `UseItemOnGolden` refuses outright on
    a gate file with no `useItemOn` gates. Gating the probe on the table being
    present made that a deadlock in any tree that had never built
    `use_item_on.json` -- the dumper needed the gates, the gates needed the
    table, and only the dumper writes the table. Measured 2026-09-23 in a fresh
    gate dir (`~/rustmc-landgp`): `ci/gen-gameplay-tables.sh` died at
    `ci/use-item-on-golden.sh` on every pass with `no useItemOn gates`. It had
    only ever worked in trees seeded by `cp -rn` from one that already had the
    table. The gates themselves are a bytecode walk and need no table; the
    populations, which do, already come back `None` for an absent table and
    say "derived only". `--check` still sweeps an optional probe only where its
    table exists.
    """
    out = dict(PROBES)
    for meth, name in OPTIONAL_PROBES.items():
        if "--emit" in sys.argv or (DATA / name).is_file():
            out[meth] = name
    return out

# # The closed vocabulary.
#
# Keyed on `Owner.method` as `javap` prints it, or a bare `method` where the
# owner is not the discriminating part. The token says **which column of which
# table would have carried the effect**, because that is the only question a
# consumer of these tables is asking.
EFFECTS = {
    # `writes` — the one token that means the recorded row is FALSE rather
    # than merely incomplete.
    "setBlock": "block-state",
    "setPoweredAndUpdate": "block-state",
    # `schedulesTick`.
    "scheduleTick": "scheduled-tick",
    # `opens` — `Player`'s screen methods are empty bodies and `ServerPlayer`
    # overrides every one, so these are gated twice over.
    "openMenu": "menu",
    "openCommandBlock": "menu",
    "openTextEdit": "menu",
    "openStructureBlock": "menu",
    "openJigsawBlock": "menu",
    "openTestBlock": "menu",
    "openTestInstanceBlock": "menu",
    "openDialog": "menu",
    "openItemGui": "menu",
    "getMenuProvider": "menu",
    "SimpleMenuProvider.\"<init>\"": "menu",
    "PlayerEnderChestContainer.setActiveChest": "menu",
    "Player.getEnderChestInventory": "menu",
    "canOpen": "menu",
    # `sound`.
    "playSound": "sound",
    # No column anywhere in these tables.
    "awardStat": "stat",
    "getOpenChestStat": "stat",
    "PiglinAi.angerNearbyPiglins": "entity-ai",
    "dropFromBlockInteractLootTable": "loot",
    "popResource": "loot",
    "gameEvent": "game-event",
    "explode": "explosion",
    "ServerPlayer.setRespawnPosition": "player-respawn",
    "ServerPlayer.getRespawnConfig": "player-respawn",
    "ServerPlayer$RespawnConfig.isSamePosition": "player-respawn",
    "ServerPlayer$RespawnConfig.\"<init>\"": "player-respawn",
    "LevelData$RespawnData.of": "player-respawn",
    "canSetSpawn": "player-respawn",
    # `blockEntityWritten` is a column, so a gate hiding a block-entity
    # **write** makes the row false where one hiding a **read** only makes it
    # incomplete. They are separate tokens for exactly that reason: mapping
    # both to one `block-entity-data` would put `SignBlockEntity.isWaxed`, a
    # read, on the same footing as `executeClickCommandsIfPresent`, which runs
    # a command.
    "BaseCommandBlock.setTrackOutput": "block-entity-write",
    "CommandBlockEntity.setAutomatic": "block-entity-write",
    "SignBlockEntity.executeClickCommandsIfPresent": "block-entity-write",
    "CommandBlockEntity.getCommandBlock": "block-entity-read",
    "SignBlockEntity.isWaxed": "block-entity-read",
    "SignBlockEntity.isFacingFrontText": "block-entity-read",
    "SignBlockEntity.getSignInteractionFailedSoundEvent": "block-entity-read",
    "SignBlockEntity.getBlockPos": "block-entity-read",
    "hasEditableText": "block-entity-read",
    "otherPlayerIsEditingSign": "block-entity-read",
    "GameRules.get": "game-rules",
    "ServerLevel.getGameRules": "game-rules",
    "hasNeighborSignal": "redstone",
    "Player.mayBuild": "player-permission",
    "Player.canUseGameMasterBlocks": "player-permission",
    "getRandom": "random",
    "nextFloat": "random",
    # Structural, and deliberately mapped to nothing: these carry no effect of
    # their own and appear on every path.
    "getBlockEntity": None,
    "getBlockState": None,
    "getValue": None,
    "setValue": None,
    "dimension": None,
    "valueOf": None,
    "intValue": None,
    "booleanValue": None,
    "GameEvent$Context.of": None,
    "BlockPos.above": None,
    "BlockPos.getX": None,
    "BlockPos.getY": None,
    "BlockPos.getZ": None,
    "ItemStack.has": None,
    "isRedstoneConductor": None,
    "Util.pauseInIde": None,
    # An `invokedynamic` is a marker, never an effect: `derive` walks the
    # `lambda$<probe>$N` body it targets and takes the tokens from there.
    "invokedynamic.accept": None,
    "invokedynamic.createMenu": "menu",
    "ChestMenu.threeRows": "menu",
    "IllegalStateException.\"<init>\"": None,

    # # THE `useItemOn` HALF OF THE VOCABULARY
    #
    # 35 calls over 7 classes that `--census` reported as unmapped and refused
    # to guess at. Two things are different on this side and both show up here.
    #
    # **There is a held stack, so the item is an axis and an outcome.** The two
    # swept probes are empty-handed or a placement, and neither can damage or
    # consume what the player is holding. `hurtAndBreak` and `consume` are the
    # tokens for that, and they are not `None`: `stackAfter` is a real column
    # of `use_item_on.json`, written whenever the stack comes back a different
    # thing, and `UseItemOnGolden` compares with `isSameItemSameComponents`
    # precisely so a damage component counts.
    #
    # **A gate can hide an entity.** `PumpkinBlock` spawns the seeds itself
    # rather than through the loot helper, so the effect is an `ItemEntity`
    # construction and an `addFreshEntity`. No column anywhere records a
    # spawn, so the token exists to be `None` rather than to be missing.
    "ItemStack.hurtAndBreak": "item-damaged",
    "ItemStack.consume": "item-consumed",
    "ItemStack.consumeAndReturn": "item-consumed",
    "ItemStack.shrink": "item-consumed",
    "dropHoneycomb": "loot",
    "CampfireBlockEntity.placeFood": "block-entity-write",
    "SignApplicator.tryApplyToSign": "block-entity-write",
    "SignApplicator.canApplyToSign": "block-entity-read",
    "SignBlockEntity.getText": "block-entity-read",
    "VaultBlockEntity$Server.tryInsertKey": "block-entity-write",
    "VaultBlockEntity.getConfig": "block-entity-read",
    "VaultBlockEntity.getServerData": "block-entity-read",
    "VaultBlockEntity.getSharedData": "block-entity-read",
    "ServerLevel.sendParticles": "particles",
    "Player.sendOverlayMessage": "player-message",
    "ItemEntity.\"<init>\"": "entity-spawn",
    "ItemEntity.setDeltaMovement": "entity-spawn",
    "Level.addFreshEntity": "entity-spawn",
    "StatType.get": "stat",
    "nextDouble": "random",
    # Structural on this side too: reads of the item, the hand and the hit
    # that decide *which* branch runs and carry no effect of their own.
    "ItemStack.is": None,
    "ItemStack.getItem": None,
    "ItemStack.isEmpty": None,
    "InteractionHand.asEquipmentSlot": None,
    "BlockHitResult.getDirection": None,
    "Player.getDirection": None,
    "Player.getX": None,
    "Player.getY": None,
    "Player.getZ": None,
    "Direction.getAxis": None,
    "Direction.getOpposite": None,
    "Direction.getStepX": None,
    "Direction.getStepZ": None,
    "Block.defaultBlockState": None,
    "Component.translatable": None,
}

# # Which token would have landed in which column, and why that is the finding.
#
# A gate hiding `PiglinAi.angerNearbyPiglins` leaves the row **incomplete**:
# nothing in either table has ever recorded entity AI, so a consumer reading
# the row is not misled. A gate hiding `openMenu` leaves the row **false**:
# `opens` is a column, eleven blocks carry it, and a chest that does not is
# saying "no screen opened" rather than "not measured".
#
# So the token maps to the column it belongs in, or to `None` for "no column in
# either table". That split is the whole verdict, and it is derived rather than
# asserted per block.
# # A COLUMN IS A PROPERTY OF (token, table), NOT OF THE TOKEN
#
# The token says which effect the gate hides. Which **column** would have
# carried it is a question about the table, and the three tables do not have
# the same columns. `opens` exists in `use_without_item.json` and in neither
# other; `stackAfter` exists in `use_item_on.json` and in neither other,
# because the two swept probes have no held stack to damage. A single global
# map would have to answer one of those wrong, and the wrong answer is the
# expensive direction: `gated: ["opens"]` on a `use_item_on.json` cell tells a
# consumer to disbelieve a field that row has never had, which reads as a
# defect in the table rather than a limit of the harness.
#
# So the map is keyed by probe. `None` means **no column in this table** — the
# gate leaves the row incomplete rather than false, which is the whole verdict
# and is derived here rather than asserted per block.
#
# `BASE_COLUMNS` is what the two swept probes share and is unchanged from when
# they were the only two; the per-probe dicts say only where they differ.
BASE_COLUMNS = {
    "block-state": "writes",
    "scheduled-tick": "schedulesTick",
    "sound": "sound",
    "random": "random",
    "menu": "opens",
    "block-entity-write": "blockEntityWritten",
    # No column in `use_without_item.json` or `set_placed_by.json`.
    "stat": None,
    "entity-ai": None,
    "loot": None,
    "explosion": None,
    "player-respawn": None,
    "block-entity-read": None,
    "game-rules": None,
    "player-permission": None,
    "game-event": None,
    "redstone": None,
    # `useItemOn`'s own tokens. Present here so a 26.3 jar that grows one of
    # these behind a `useWithoutItem` gate gets a column rather than an
    # `UNMAPPED`, and `None` because neither swept table has the field.
    "item-damaged": None,
    "item-consumed": None,
    "particles": None,
    "player-message": None,
    "entity-spawn": None,
}

COLUMNS = {
    "setPlacedBy": dict(BASE_COLUMNS),
    "useWithoutItem": dict(BASE_COLUMNS),
    # Three differences, each of them measured against what
    # `tools/goldengen/vanilla/UseItemOnGolden.java` actually appends:
    #
    #  * **`opens` is not a field of this table.** `useItemOn` runs at offset
    #    145 of `ServerPlayerGameMode.useItemOn` and the menu-opening rules
    #    are all on the empty-handed side at 196, so the dumper never grew the
    #    column. A hidden `openMenu` here leaves the row incomplete.
    #  * **`blockEntityWritten` is not a field of this table either.**
    #    `UseGolden` and `PlacedGolden` both track `beWritten` off their
    #    level; `UseItemOnGolden` does not, and four of the seven rules here
    #    hide a block-entity write. That is the largest single thing this
    #    population is missing and it is named as `None` rather than pointed
    #    at a field that does not exist.
    #  * **`stackAfter` exists and the other two have no equivalent.** The
    #    held stack is an input on this side, so damaging or consuming it is
    #    an outcome the row records.
    "useItemOn": dict(BASE_COLUMNS, **{
        "menu": None,
        "block-entity-write": None,
        "item-damaged": "stackAfter",
        "item-consumed": "stackAfter",
    }),
}


def columns_for(probe, hides):
    """The columns of `probe`'s table that `hides` would have landed in."""
    table = COLUMNS[probe]
    return sorted({table[h] for h in hides if table.get(h)})


# # THE POPULATION THIS FILE DOES NOT SWEEP, AND WILL NOT PRETEND TO
#
# `useItemOn` is the other half of the right-click and its table,
# `use_item_on.json`, is the optional fifth population: nothing under `crates/`
# opens it and its dumper is ~25 minutes, so most trees do not have it. Wiring
# it into `PROBES` in a tree without the table would make this check refuse
# everywhere.
#
# But the gates are in the jar whether the table exists or not, and a number
# derived is worth more to whoever prices that work than a number written into
# a doc by hand. `--census` reports them and asserts nothing: no ledger, no
# columns, no stamping. The `hides` it prints is **best-effort**, because the
# vocabulary was closed against the two swept probes and `useItemOn` reaches
# calls it has never been given a token for — which is the closed vocabulary
# working, and exactly the list the next lane has to work through.
# `--census` is what this was before it was swept, and it stays: in a tree
# with no table it is the only way to see the population at all, and it needs
# no ledger, no columns and no stamping. It reports the same set `probes()`
# would have added had the table been there.
CENSUS = dict(OPTIONAL_PROBES)

BRANCH = {
    "ifeq", "ifne", "iflt", "ifge", "ifgt", "ifle",
    "if_icmpeq", "if_icmpne", "if_icmplt", "if_icmpge", "if_icmpgt", "if_icmple",
    "if_acmpeq", "if_acmpne", "ifnull", "ifnonnull",
}
TERMINAL = {"return", "areturn", "ireturn", "lreturn", "freturn", "dreturn",
            "athrow"}


def jar():
    ver = (ROOT / "crates" / "rustmc-proto" / "src" / "lib.rs").read_text()
    ver = ver.split('GAME_VERSION: &str = "')[1].split('"')[0]
    return ROOT / "oracle-work" / f"vanilla-{ver}" / "versions" / ver \
        / f"server-{ver}-unsigned.jar"


def declaring_classes(path):
    """`{method: {simple class name}}` for every class declaring a probe method.

    Deliberately the same derivation as `ci/probe-absences.py`: the whole
    `world/level/block` package **recursively**, and `Block` discarded because
    it declares both and overrides neither.
    """
    with zipfile.ZipFile(path) as z:
        names = [n[:-6].replace("/", ".") for n in z.namelist()
                 if n.startswith(PKG) and n.endswith(".class")]
    out = subprocess.run(["javap", "-p", "-cp", str(path), *sorted(names)],
                         capture_output=True, text=True, check=True).stdout
    found = {m: set() for m in probes()}
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
    for m in found:
        found[m].discard("Block")
    return found


def qualified(path, want):
    with zipfile.ZipFile(path) as z:
        index = {n[:-6].rsplit("/", 1)[-1]: n[:-6].replace("/", ".")
                 for n in z.namelist()
                 if n.startswith(PKG) and n.endswith(".class")}
    return [index[c] for c in want if c in index]


def disassemble(path, want):
    """`javap -c` over exactly the classes that declare a probe method."""
    names = [PKG.replace("/", ".") + c for c in want]
    out = subprocess.run(["javap", "-p", "-c", "-cp", str(path), *names],
                         capture_output=True, text=True).stdout
    if "Error:" in out:
        out = subprocess.run(["javap", "-p", "-c", "-cp", str(path),
                              *qualified(path, want)],
                             capture_output=True, text=True).stdout
    return out


INSN = re.compile(r"^\s+(\d+):\s+(\S+)\s*(.*?)\s*$")
# `javap` prints every member at **exactly** two-space indent, `Code:` at four
# and instructions at seven, so two-space-exact is an unambiguous member
# boundary. Matching only signatures with parentheses is not: `static {};` has
# none, and its `<clinit>` instructions then land in whichever method was
# parsed last — which is how `Block.column` and `simpleCodec`, both field
# initialisers, first appeared inside `SweetBerryBushBlock.useWithoutItem`.
MEMBER = re.compile(r"^  \S.*;\s*$")


def bodies(text):
    """`{(class, method name): [(offset, opcode, rest), ...]}` for every method.

    **Every** method, not only the probe ones, because a `lambda$...` body is
    where an effect can hide: `SweetBerryBushBlock`'s gated region calls
    `dropFromBlockInteractLootTable` with an `invokedynamic` whose lambda calls
    `Block.popResource`. Walking only the probe method sees an
    `invokedynamic #0:accept` and nothing else, and a vocabulary that maps that
    to a token is asserting something about a body it never read.
    """
    out, cls, meth, code = {}, None, None, []

    def close():
        if cls and meth and code:
            out[(cls, meth)] = list(code)

    for line in text.splitlines():
        if line and not line[0].isspace() and "class " in line and "(" not in line:
            close()
            cls = line.split("class ", 1)[1].split("<")[0].split(" extends")[0] \
                      .split(" implements")[0].strip().rstrip("{").strip() \
                      .split(".")[-1]
            meth, code = None, []
        elif MEMBER.match(line):
            close()
            code = []
            if "(" in line:
                head = line.split("(", 1)[0].strip()
                meth = head.rsplit(" ", 1)[-1].split(".")[-1] if " " in head \
                    else head
            else:
                # a field, or `static {};` — a member, and not a method body
                # this derivation may attribute instructions to.
                meth = None
        elif meth is not None:
            m = INSN.match(line)
            if m and line.startswith("      "):
                code.append((int(m.group(1)), m.group(2), m.group(3)))
    close()
    return out


def target(rest):
    """The branch target `javap` prints as the operand, or None."""
    m = re.match(r"^(-?\d+)", rest)
    return int(m.group(1)) if m else None


def reachable(code, start):
    """Offsets reachable from `start` by following the control flow.

    A conditional branch has two successors, a `goto` one, a return or a throw
    none. `tableswitch`/`lookupswitch` take every printed target — none of the
    eight gates has one, and a jar that grows one must not silently lose its
    successors.
    """
    at = {off: i for i, (off, _, _) in enumerate(code)}
    seen, stack = set(), [start]
    while stack:
        off = stack.pop()
        if off in seen or off not in at:
            continue
        seen.add(off)
        i = at[off]
        _, op, rest = code[i]
        nxt = code[i + 1][0] if i + 1 < len(code) else None
        if op in TERMINAL:
            continue
        if op == "goto" or op == "goto_w":
            t = target(rest)
            if t is not None:
                stack.append(t)
            continue
        if op in BRANCH:
            t = target(rest)
            if t is not None:
                stack.append(t)
            if nxt is not None:
                stack.append(nxt)
            continue
        if op in ("tableswitch", "lookupswitch"):
            for t in re.findall(r":\s*(\d+)", rest):
                stack.append(int(t))
            continue
        if nxt is not None:
            stack.append(nxt)
    return seen


def gate_of(code):
    """`(instanceof offset, ifeq offset, ifeq target)` for the ServerLevel gate.

    The `instanceof` whose constant-pool comment names `ServerLevel`, and the
    conditional that consumes its result. A body may mention `ServerLevel` in a
    descriptor without branching on it — `PiglinAi.angerNearbyPiglins
    (ServerLevel, ...)` is a call, not a gate — so the `instanceof` opcode is
    what is matched, never the string.
    """
    at = {off: i for i, (off, _, _) in enumerate(code)}
    for off, op, rest in code:
        if op != "instanceof" or f"// class {SERVER_LEVEL}" not in rest:
            continue
        i = at[off]
        for j in range(i + 1, min(i + 4, len(code))):
            off2, op2, rest2 = code[j]
            if op2 in BRANCH:
                return off, off2, target(rest2)
    return None


def invocations(code, offsets):
    """`{Owner.method}` for every call at one of `offsets`.

    `javap` prints `// Method net/minecraft/.../Owner.name:(...)V`. The owner is
    kept because `setBlock` on a `ServerLevel` and `getValue` on a `BlockState`
    are not the same fact, and dropped to the simple name for the lookup so the
    vocabulary does not have to spell out a package per entry.
    """
    out = set()
    for off, op, rest in code:
        if off not in offsets or not op.startswith("invoke"):
            continue
        m = re.search(r"// (?:Method|InterfaceMethod) ([^:]+):", rest)
        if m:
            out.add(m.group(1).replace("/", "."))
            continue
        m = re.search(r"// InvokeDynamic #\d+:([^:]+):", rest)
        if m:
            # Kept as a marker rather than a token. `derive` unions the class's
            # `lambda$<probe>$N` bodies in when one of these is gated, so the
            # effect comes from the lambda's own instructions.
            out.add("invokedynamic." + m.group(1))
    return out


def token(ref):
    """The vocabulary token for one invocation, or `KeyError` if unmapped."""
    simple = ref.rsplit(".", 1)[-1]
    owner = ref.rsplit(".", 2)
    pair = ".".join(owner[-2:]) if len(owner) >= 2 else simple
    for key in (pair, simple):
        if key in EFFECTS:
            return EFFECTS[key]
    raise KeyError(ref)


# # THE GATE THAT IS NOT IN THE RULE, AND WHY THE CFG WALK CANNOT SEE IT
#
# `TntBlock.useItemOn` is the counter-example to this file's own derivation.
# Its body is
#
#     if (!stack.is(FLINT_AND_STEEL) && !stack.is(FIRE_CHARGE))
#         return super.useItemOn(...);          //  20-35
#     if (prime(level, pos, player)) {          //  36-44
#         level.setBlock(pos, AIR, 11);         //  47-61
#         ...consume or damage the item, awardStat
#     } else if (level instanceof ServerLevel serverLevel) {   // 116-120
#         if (!gameRules.get(TNT_EXPLODES)) { sendOverlayMessage; return PASS; }
#     }
#     return SUCCESS;                           // 163
#
# The `instanceof` this file finds is the one at 116, and what it hides is a
# game-rule read and an overlay message — neither of which is a column. On
# that reading `minecraft:tnt` is an incomplete row and nothing more.
#
# It is not. `TntBlock.prime` is **itself** `instanceof ServerLevel` at its
# own offset 0 and returns `false` when it is not one, so in this harness the
# `setBlock(pos, AIR, 11)` at 47 is unreachable — and 47 is on the caller's
# fall-through, which the walk classifies as *not gated* because the gate it
# knows about is 69 bytes further on. The row says
# `result: SUCCESS, consumes: true, writes: []`, and the write it is missing
# is the whole interaction.
#
# **The subtraction cannot find this.** `prime` is called at 41, before the
# branch, so it is reachable from both sides and cancels — correctly, by the
# rule that makes the join work. The effect is one level down a call, and a
# walk of one body is the wrong shape for it however carefully it is done.
#
# So this is a **detection**, not a derivation: every call a probe rule makes
# into a body this run disassembled is checked for a gate of its own, and one
# that has it is reported as `via`. What the caller then does with the
# constant return value is a dataflow question no bytecode walk here answers,
# which is why the ledger owes each one a `via_why` in prose rather than a
# token. Refusing an unledgered `via` is the point: a rule that grows one in
# 26.3 must not land as a quietly incomplete `hides`.
def gated_callees(cls, meth, code, all_bodies):
    """`{callee: [what its own gate hides]}` for calls that are gated too.

    Only bodies this run disassembled, which is every class declaring a probe
    method — `prime` is a private static of `TntBlock` and so is in reach. A
    helper on some *other* class is not, and that limit is real and stated
    rather than papered over: `--census` prints the count it checked.
    """
    out = {}
    for off, op, rest in code:
        if not op.startswith("invoke"):
            continue
        m = re.search(r"// (?:Method|InterfaceMethod) ([^:]+):", rest)
        if not m:
            continue
        ref = m.group(1).replace("/", ".")
        name = ref.rsplit(".", 1)[-1]
        owner = ref.rsplit(".", 2)[0].split(".")[-1] if "." in ref else cls
        for key in ((cls, name), (owner, name)):
            # `super.useItemOn(...)` resolves to `Block.useItemOn` and the
            # simple-name lookup lands back on the rule itself, which is gated
            # by construction. Every one of the eight swept rules and three of
            # the seven here reported themselves before this line existed —
            # a "finding" that is only the walk seeing its own reflection.
            if key == (cls, meth):
                continue
            body = all_bodies.get(key)
            if body is None or gate_of(body) is None:
                continue
            g = gate_of(body)
            fall = None
            idx = [o for o, _, _ in body].index(g[1])
            if idx + 1 < len(body):
                fall = body[idx + 1][0]
            taken = reachable(body, fall) if fall is not None else set()
            not_taken = reachable(body, g[2]) if g[2] is not None else set()
            hides = set()
            for r in invocations(body, taken - not_taken):
                try:
                    tok = token(r)
                except KeyError:
                    tok = None
                if tok:
                    hides.add(tok)
            out[f"{key[0]}.{name}"] = sorted(hides)
            break
    return out


def guarded(code, gate_at):
    """Whether any conditional branch precedes the gate.

    This is the half of "which cells reach the gate" that the bytecode can
    settle on its own, and it is what pins the ledger's `guard` honest: no
    conditional before the gate means every cell reaches it and the ledger must
    say `always`.
    """
    return any(op in BRANCH or op in ("goto", "goto_w")
               for off, op, _ in code if off < gate_at)


def derive():
    """`{(class, method): {...}}` for every probe rule gated on ServerLevel."""
    path = jar()
    found = declaring_classes(path)
    want = sorted({c for m in found for c in found[m]})
    text = disassemble(path, want)
    all_bodies = bodies(text)
    out, unmapped = {}, []
    for (cls, meth), code in sorted(all_bodies.items()):
        if cls not in found.get(meth, ()):
            continue
        g = gate_of(code)
        if not g:
            continue
        gate_at, branch_at, branch_to = g
        i = [off for off, _, _ in code].index(branch_at)
        fallthrough = code[i + 1][0] if i + 1 < len(code) else None
        taken = reachable(code, fallthrough) if fallthrough is not None else set()
        not_taken = reachable(code, branch_to) if branch_to is not None else set()
        hides, announces = set(), False
        refs = invocations(code, taken - not_taken)
        # A gated `invokedynamic` means a lambda runs on the far side of the
        # gate. Union its own instructions in, or `Block.popResource` behind
        # `SweetBerryBushBlock`'s loot drop is an effect nothing ever read.
        if any(r.startswith("invokedynamic.") for r in refs):
            for (c2, m2), code2 in all_bodies.items():
                if c2 == cls and m2.startswith(f"lambda${meth}$"):
                    refs |= invocations(code2, {o for o, _, _ in code2})
        for ref in sorted(refs):
            try:
                tok = token(ref)
            except KeyError:
                unmapped.append((cls, meth, ref))
                continue
            if tok:
                hides.add(tok)
        for ref in invocations(code, not_taken):
            if ref.endswith("." + PAUSE):
                announces = True
        out[(cls, meth)] = {
            "hides": sorted(hides),
            "announces": announces,
            "guarded": guarded(code, gate_at),
            "via": gated_callees(cls, meth, code, all_bodies),
        }
    return out, unmapped


def census(probe):
    """`(gates, blocks, unmapped)` for a probe this file does not sweep.

    Same derivation as `derive`, with the table half removed and `token`
    failures collected instead of raised.
    """
    path = jar()
    found = {probe: set()}
    with zipfile.ZipFile(path) as z:
        names = [n[:-6].replace("/", ".") for n in z.namelist()
                 if n.startswith(PKG) and n.endswith(".class")]
    out = subprocess.run(["javap", "-p", "-cp", str(path), *sorted(names)],
                         capture_output=True, text=True, check=True).stdout
    cur = None
    for line in out.splitlines():
        if "class " in line and line[:1] not in (" ", "\t") and "(" not in line:
            cur = line.split("class ", 1)[1].split("<")[0].split(" extends")[0] \
                      .split(" implements")[0].strip().rstrip("{").strip() \
                      .split(".")[-1]
        elif f" {probe}(" in line:
            found[probe].add(cur)
    found[probe].discard("Block")
    want = sorted(found[probe])
    all_bodies = bodies(disassemble(path, want))
    gates, unmapped = {}, []
    for (cls, meth), code in sorted(all_bodies.items()):
        if meth != probe or cls not in found[probe]:
            continue
        g = gate_of(code)
        if not g:
            continue
        gate_at, branch_at, branch_to = g
        i = [off for off, _, _ in code].index(branch_at)
        fall = code[i + 1][0] if i + 1 < len(code) else None
        taken = reachable(code, fall) if fall is not None else set()
        not_taken = reachable(code, branch_to) if branch_to is not None else set()
        hides, announces = set(), False
        refs = invocations(code, taken - not_taken)
        if any(r.startswith("invokedynamic.") for r in refs):
            for (c2, m2), code2 in all_bodies.items():
                if c2 == cls and m2.startswith(f"lambda${meth}$"):
                    refs |= invocations(code2, {o for o, _, _ in code2})
        for ref in sorted(refs):
            try:
                tok = token(ref)
            except KeyError:
                unmapped.append((cls, ref))
                continue
            if tok:
                hides.add(tok)
        for ref in invocations(code, not_taken):
            if ref.endswith("." + PAUSE):
                announces = True
        gates[cls] = {"hides": sorted(hides), "announces": announces,
                      "via": gated_callees(cls, meth, code, all_bodies)}
    reg = registry()
    blocks = ({c: blocks_under(c, *reg) for c in gates} if reg
              else {c: None for c in gates})
    return gates, blocks, unmapped


# # THE ONE FILE THAT CARRIES THE CLASS CHAIN, AND THE DEADLOCK THAT WAS
#
# `set_placed_by.json` is where the block -> class -> superclass chain lives,
# so every count in this file is downstream of it. It is also **generated by
# `ci/placed-golden.sh`, which runs `--emit` first** — so in a tree that has
# never built it, `--emit` refused on a missing table that only the dumper it
# was gating could produce. A fresh worktree could not run either dumper at
# all, and the error it printed named a script that does not break the cycle.
#
# `--emit` writes nothing that needs the table: `hides`, `columns`,
# `announces` and the ledger's `guard` are all derived from the jar. So the
# registry is optional, `populations` leaves the measured fields `None` when it
# is absent, and every arm that reads one is skipped rather than being answered
# with a zero. `--check` still refuses outright, because a check that silently
# drops its population arms is the unfalsifiable shape this whole area exists
# to stop.
def registry():
    """`(classes, superclasses)`, or `None` in a tree with no table yet."""
    path = DATA / "set_placed_by.json"
    if not path.is_file():
        return None
    doc = json.loads(path.read_text())
    return doc["classes"], doc["superclasses"]


def blocks_under(cls, classes, supers):
    def chain(n):
        return [n] + supers.get(n, [])
    return sorted(b for b, cn in classes.items() if cls in chain(cn))


def load():
    if not LEDGER.is_file():
        return {}, {}
    doc = tomllib.loads(LEDGER.read_text())
    return ({(e["probe"], e["class"]): e for e in doc.get("gate", [])},
            doc.get("meta", {}))


_USABLE = {}


def table_usable(name):
    """A present table that is STALE under its own provenance stamp is not a
    measurement of this tree, and under `--emit` it is treated as absent.

    Found on the first fresh gate of the stacked branch (tree `d2b54924c`,
    box `rustmc-stackgp`, 2026-09-04): `ci/gen-gameplay-tables.sh` regenerates
    stale tables in its own fixed order, `set_placed_by.json` first, and
    `ci/placed-golden.sh` runs `--emit` before it writes anything. `--emit`
    then measured the tree's *previous* `use_item_on.json` -- generated by the
    old probe, with no sign rows -- against a ledger written from the new one,
    and refused with `LIVE SignBlock.useItemOn -- ledger 2784 != measured 0`
    and ten more lines like it. Every one of those tables was about to be
    regenerated by the same script two steps later, so the refusal was of a
    file the tree had already declared stale. A table whose stamp does not
    match its generator is evidence of nothing here; `--check`, which runs
    after the dumpers, still reads every table it finds.

    `ci/provenance.sh` is the one place the staleness rule lives, so it is
    asked rather than re-implemented.
    """
    if "--emit" not in sys.argv:
        return True
    if name in _USABLE:
        return _USABLE[name]
    r = subprocess.run(
        ["bash", "-c", 'source "$1/ci/provenance.sh" && prov_check "$1" "$2"',
         "_", str(ROOT), str(DATA / name)],
        capture_output=True, text=True)
    ok = r.returncode == 0
    if not ok:
        print(f"server-level-gates --emit: {name} is {r.stdout.strip() or 'not current'} "
              f"-- treated as absent, ci/gen-gameplay-tables.sh regenerates it",
              file=sys.stderr)
    _USABLE[name] = ok
    return ok


def populations(gates):
    """Per rule: the blocks under it, and what the table already says.

    `live` is the number of recorded cells that are **not** refusals, which is
    the population whose row was decided by the gate rather than by the block.
    For `SignBlock` it is 0 — vanilla announces, `UseGolden` refuses, and 1,344
    cells say so — and that difference is the entire reason the other seven are
    a finding.
    """
    reg = registry()
    tables = {}
    out = {}
    for (cls, meth), d in sorted(gates.items()):
        cols = columns_for(meth, d["hides"])
        name = probes()[meth]
        # A tree with no `set_placed_by.json` has no class chain and therefore
        # no population. The derived half is still complete, and `None` is what
        # says so — a zero here would read as "no blocks under this rule",
        # which is a measurement, and none was taken.
        if reg is None or not (DATA / name).is_file() or not table_usable(name):
            out[(cls, meth)] = dict(
                d, blocks=None, rows=None, cells=None, refused=None,
                live=None, gated=None, columns=cols, block_ids=None)
            continue
        classes, supers = reg
        if name not in tables:
            tables[name] = json.loads((DATA / name).read_text())
        doc = tables[name]
        table = {r["block"]: r for r in doc["table"]}
        blocks = blocks_under(cls, classes, supers)
        have = [b for b in blocks if b in table]
        cells, refused = count_cells(doc, set(have))
        # `live` is every recorded, unrefused cell under the rule. `gated` is
        # the subset the guard admits — the cells the branch actually decided
        # — and it is the one the dumper's stamp must equal. On the swept
        # probes the two are the same number for seven of the eight rules and
        # the eighth, `SweetBerryBushBlock`, only by the accident that its
        # ungated states had no interesting cell to record. The item axis
        # ends that: `minecraft:pumpkin` records two cells and one shears.
        ledger, _ = load()
        guard = (ledger.get((meth, cls), {}) or {}).get("guard")
        try:
            admitted = (gated_cells(doc, set(have), guard) if guard else None)
        except ValueError:
            # A guard the table's own strings cannot answer is a GUARD arm's
            # job to report, not something to turn into a wrong number here.
            admitted = None
        out[(cls, meth)] = dict(
            d, blocks=len(blocks), rows=len(have), cells=cells,
            refused=refused, live=cells - refused, gated=admitted,
            columns=cols, block_ids=have)
    return out


def totals(pops, only=None):
    """The three numbers this file exists to publish, over `only`'s probes.

    Every population figure is `None` in a tree with no table, because it was
    not measured. `--check` never sees that state; `--emit` does, and a zero
    would be a claim.
    """
    if only is not None:
        pops = {k: v for k, v in pops.items() if k[1] in only}
    silent = {k: v for k, v in pops.items() if not v["announces"]}
    measured = all(v["blocks"] is not None for v in pops.values())
    if not measured:
        return {"rules": len(pops), "blocks": None,
                "silent_rules": len(silent), "silent_blocks": None,
                "false_cells": None, "false_blocks": None}
    return {
        "rules": len(pops),
        "blocks": sum(v["blocks"] for v in pops.values()),
        "silent_rules": len(silent),
        "silent_blocks": sum(v["blocks"] for v in silent.values()),
        # A row that exists, carries cells, and is wrong in a column the table
        # has. Not "missing a row" and not "refused" — the third thing.
        #
        # Counted over `gated` and not `live`: a cell the guard excludes was
        # not decided by the branch and is not false. The two are the same
        # number for all eight swept rules and differ for four of the seven
        # on `useItemOn`, which is the item axis showing up in the total.
        "false_cells": sum(v["gated"] or 0 for v in silent.values()
                           if v["columns"]),
        "false_blocks": sum(len(v["block_ids"]) for v in silent.values()
                            if v["columns"] and v["gated"]),
    }


def emit(pops):
    """The dumpers' input. **Derived here, applied there.**

    `UseGolden` and `PlacedGolden` hold the real `BlockState` and can say which
    cells reach the gate; only the bytecode can say what the gate hides. So the
    fact is derived once, here, and written where both dumpers read it, rather
    than each transcribing a list of class names it cannot check.
    """
    ledger, _ = load()
    doc = {
        "_comment": "GENERATED by ci/server-level-gates.py --emit. "
                    "Do not edit; see ci/checks/server-level-gates.toml.",
        "gates": [],
    }
    for (cls, meth), v in sorted(pops.items()):
        e = ledger.get((meth, cls), {})
        doc["gates"].append({
            "probe": meth,
            "class": cls,
            "hides": v["hides"],
            "columns": v["columns"],
            "announces": v["announces"],
            "via": sorted(v.get("via", {})),
            "guard": e.get("guard", "no-branch"),
            "guard_why": e.get("guard_why", ""),
            "verdict": e.get("verdict", "unverdicted"),
        })
    EMIT.parent.mkdir(parents=True, exist_ok=True)
    EMIT.write_text(json.dumps(doc, indent=2) + "\n")
    return EMIT


# # THE GUARD GRAMMAR, AND WHY IT GREW AN ITEM AXIS
#
# A guard says which cells reach the gate. For the two swept probes a cell is
# a `BlockState` and the vocabulary only ever had to name an integer property
# — `age>1` on the berry bush, `charges>0` on the respawn anchor.
#
# A `use_item_on.json` cell is a **(state, item)** pair, and five of the seven
# rules here branch on the item before they reach the gate:
# `PumpkinBlock` on `stack.is(SHEARS)`, `TntBlock` on flint and steel or a
# fire charge, `VaultBlock` on a non-empty stack *and* `vault_state=active`,
# `DecoratedPotBlock` on a non-empty stack, `CampfireBlock` on whether the
# stack is a campfire recipe input. A state-only guard has to answer every one
# of those `all-states`, which would stamp the whole grid — for
# `minecraft:pumpkin` that is 1,537 cells claiming to be decided by a branch
# that 1,536 of them never reach.
#
# So a guard is a conjunction of terms over two axes:
#
#     no-branch                the gate is the body's first branch
#     all-states               something branches first, nothing state- or
#                              item-dependent in this harness (owes guard_why)
#     <prop><op><int>          honey_level>4          — integer property
#     <prop>=<value>           vault_state=active     — enum property
#     item=<id>[,<id>...]      item=minecraft:shears  — a named item set
#     item-any                 any non-empty stack
#     item-unknown             the branch is on something the dumper cannot
#                              evaluate; **stamps nothing** and owes guard_why
#
# `item-unknown` is the honest floor and it is not a shrug: `CampfireBlock`'s
# branch is `recipeAccess().propertySet(CAMPFIRE_INPUT).test(stack)`, this
# harness refuses `recipeAccess` outright, and all 64 campfire states are
# refusals rather than rows. A refused cell is never stamped, so the term
# stamping nothing is the same answer arrived at from the other side — and it
# is checked, rather than being true by accident of the refusal.
GUARD_TERM = re.compile(
    r"^(?:no-branch|all-states|item-any|item-unknown"
    r"|item=[a-z0-9_:]+(?:,[a-z0-9_:]+)*"
    r"|[a-z_]+[<>=][a-z0-9_]+)$")
GUARD_NEEDS_WHY = {"all-states", "item-unknown"}


def guard_terms(guard):
    return [t.strip() for t in str(guard).split("&&")]


def guard_bad(guard):
    """Why this guard is not a guard, or `None`."""
    if not guard:
        return "missing"
    terms = guard_terms(guard)
    for t in terms:
        if not GUARD_TERM.match(t):
            return f"{t!r} is not a guard term"
    if "no-branch" in terms and len(terms) > 1:
        return "`no-branch` is the whole guard or it is not true"
    return None


# # EVALUATING THE GUARD HERE TOO, AND WHY THAT IS NOT DUPLICATION
#
# The dumper evaluates the guard against the real `BlockState` and the real
# `ItemStack`. This evaluates it against the **strings the table printed** —
# `minecraft:beehive[facing=north,honey_level=5]` and the item ids in an
# `except` group. Two routes to the same number, one through the JVM's own
# objects and one through the serialised text, and `--check` refuses if they
# disagree.
#
# That is what the STAMP-DERIVED arm needs to stay honest here. On the swept
# probes it could compare the stamp against every recorded cell of the rule,
# because the one state guard in play — `age>1` on the berry bush — happened
# to exclude exactly the states that had no interesting cell anyway. On
# `useItemOn` that coincidence is gone: `minecraft:pumpkin` records two cells
# and only one of them is a shears cell, so "every recorded cell" over-counts
# the gated population by the width of the item axis.
def reaches_cell(guard, state, item=None):
    """Whether the cell `(state, item)` reaches the gate, per `guard`.

    `state` is the serialised `Block[prop=value,...]` the table printed and
    `item` the id of the `except` group, or `None` for a table with no item
    axis. An `item-` term over a cell with no item is a ledger error and
    raises rather than defaulting either way.
    """
    # Validated before it is evaluated, not term by term inside the loop. An
    # unreadable guard has exactly one honest outcome — GUARD-SYNTAX, naming
    # the ledger entry — and reaching the evaluator with it produces whatever
    # the first mismatched branch happens to throw. `item~minecraft:shears`
    # took the item branch on its `item` prefix and died in a `split`.
    why = guard_bad(guard)
    if why:
        raise ValueError(f"{guard!r}: {why}")
    props = {}
    if "[" in state:
        for kv in state.split("[", 1)[1].rstrip("]").split(","):
            if "=" in kv:
                k, v = kv.split("=", 1)
                props[k.strip()] = v.strip()
    for t in guard_terms(guard):
        if t in ("no-branch", "all-states"):
            continue
        if t == "item-unknown":
            return False
        if t.startswith("item"):
            if item is None:
                raise ValueError(f"{t!r} on a table with no item axis")
            if t == "item-any":
                if item == "minecraft:air":
                    return False
                continue
            want = set(t.split("=", 1)[1].split(","))
            if item not in want:
                return False
            continue
        m = re.match(r"^([a-z_]+)([<>=])([a-z0-9_]+)$", t)
        if m is None:
            # Not an `AttributeError` on `m.group`: `populations` catches a
            # ValueError and leaves the count unmeasured so that GUARD-SYNTAX
            # can report the real fault, where a traceback takes the whole
            # check down and says nothing about which ledger entry is wrong.
            raise ValueError(f"{t!r} is not a guard term")
        have = props.get(m.group(1))
        if have is None:
            raise ValueError(f"{state} has no property {m.group(1)!r}")
        op, want = m.group(2), m.group(3)
        if op == "=":
            if have != want:
                return False
        else:
            if not want.lstrip("-").isdigit() or not have.lstrip("-").isdigit():
                raise ValueError(f"{t!r} compares {have!r} with an ordering "
                                 f"and one of them is not an integer")
            if op == ">" and not int(have) > int(want):
                return False
            if op == "<" and not int(have) < int(want):
                return False
    return True


# The default cell of a `use_item_on.json` state stands for **every item the
# table did not name**, and it is one cell however many items that is. So the
# guard is asked about it with a sentinel that is deliberately not any real
# id: `item-any` admits it (an unnamed item is a non-empty stack that is not
# the empty-hand group), `item=<ids>` rejects it (every id a guard names has
# its own group, which is why it differs from the default at all), and
# `item-unknown` rejects it like everything else.
UNNAMED_ITEM = "\u0000unnamed"


def count_cells(doc, block_ids):
    """`(cells, refused)` over these blocks, counting **cells** not states.

    `use_without_item.json` and `set_placed_by.json` print one object per
    state and the two words are interchangeable there. `use_item_on.json`
    prints a `default` plus `except` groups over items, so a state is a row of
    the grid and not a point in it — counting states there called
    `minecraft:pumpkin` one cell and `minecraft:oak_sign` 1,344, when the
    figures are 2 and 3,072.
    """
    cells = refused = 0
    for row in doc["table"]:
        if row["block"] not in block_ids:
            continue
        for st in row["states"]:
            if "default" not in st:
                cells += 1
                refused += 1 if "refused" in st else 0
                continue
            cells += 1
            refused += 1 if "refused" in st["default"] else 0
            for g in st.get("except", []):
                cells += 1
                refused += 1 if "refused" in g["cell"] else 0
    return cells, refused


def gated_cells(doc, block_ids, guard):
    """Recorded, unrefused cells of these blocks that the guard admits.

    Handles both cell shapes: `use_without_item.json` and `set_placed_by.json`
    print one object per state, `use_item_on.json` prints a `default` plus
    `except` groups over items.
    """
    n = 0
    for row in doc["table"]:
        if row["block"] not in block_ids:
            continue
        for st in row["states"]:
            if "refused" in st:
                continue
            if "default" not in st:
                if reaches_cell(guard, st["from"]):
                    n += 1
                continue
            if ("refused" not in st["default"]
                    and reaches_cell(guard, st["from"], UNNAMED_ITEM)):
                n += 1
            for g in st.get("except", []):
                if "refused" in g["cell"]:
                    continue
                # **One group is one cell.** A group is a single recorded
                # answer that a set of items shares, it is the object a
                # consumer reads and it is the object that will carry
                # `gated`, so counting its items would count the same stamp
                # sixteen times for a sign's dyes. This is a different figure
                # from the dumper's own `cells` summary, which counts
                # (state, item) grid points that differ from their default —
                # 44,078 of them — and the two are not interchangeable.
                if any(reaches_cell(guard, st["from"], i) for i in g["items"]):
                    n += 1
    return n


VERDICTS = {
    # The branch is something rustmc will implement, and the hidden effect
    # belongs in a column this table already has. Real debt, counted as such.
    "reachable",
    # The branch needs a subsystem this project has none of. The row cannot be
    # completed until that exists, and saying so is the honest record.
    "needs-subsystem",
    # The gate is already loud: vanilla announces it and the dumper refuses.
    "announced",
}


def check(gates, pops, unmapped, tables=True):
    ledger, meta = load()
    bad = []
    for cls, meth, ref in unmapped:
        bad.append(("UNMAPPED", f"{cls}.{meth}",
                    f"{ref} is behind the gate and has no token in EFFECTS — "
                    f"the vocabulary is closed on purpose, give it one"))
    for (cls, meth), v in sorted(pops.items()):
        e = ledger.get((meth, cls))
        if not e:
            bad.append(("UNLEDGERED", f"{cls}.{meth}",
                        f"gated on ServerLevel over {v['blocks']} registry "
                        f"block(s) with no entry in {LEDGER.name}"))
            continue
        if e.get("verdict") not in VERDICTS:
            bad.append(("VERDICT", f"{cls}.{meth}",
                        f"{e.get('verdict')!r} is not one of "
                        f"{sorted(VERDICTS)}"))
        if bool(e.get("announces")) != v["announces"]:
            bad.append(("ANNOUNCES", f"{cls}.{meth}",
                        f"ledger says announces={e.get('announces')}, the jar "
                        f"says {v['announces']} — if vanilla stopped calling "
                        f"pauseInIde the dumper's refusals just became rows"))
        if sorted(e.get("hides", [])) != v["hides"]:
            bad.append(("HIDES", f"{cls}.{meth}",
                        f"ledger {sorted(e.get('hides', []))} != jar "
                        f"{v['hides']}"))
        if sorted(e.get("columns", [])) != v["columns"]:
            bad.append(("COLUMNS", f"{cls}.{meth}",
                        f"ledger {sorted(e.get('columns', []))} != derived "
                        f"{v['columns']}"))
        for key in ("blocks", "rows", "live"):
            if key in e and v[key] is not None and e[key] != v[key]:
                bad.append((key.upper(), f"{cls}.{meth}",
                            f"ledger {e[key]} != measured {v[key]}"))
        # The half of `guard` the bytecode settles: whether anything branches
        # before the gate at all. Which *states* reach it needs the real
        # `BlockState` and is the dumper's job; this pins the ledger honest
        # about whether the question even arises.
        # An unledgered `via` is a refusal, and the reason is that its
        # `hides` **under-reports on purpose**. `TntBlock.prime` hides the
        # spawn and the sound it performs itself; the `setBlock(pos, AIR, 11)`
        # that the caller runs on the strength of its return value is not in
        # any of these sets, because which of the caller's instructions a
        # constant return value makes unreachable is a dataflow question and
        # nothing here answers it. So the ledger owes prose, not a token.
        # # ZERO STAMPED CELLS HAS TWO CAUSES AND THEY MUST NOT LOOK ALIKE
        #
        # A rule can stamp nothing because its cells genuinely do not exist —
        # `BeehiveBlock`'s shears cell folded into the default, because a cell
        # that does nothing *is* the default — or because the stamping
        # mechanism resolved no block, matched no class or evaluated a guard
        # that selects nothing. The second is the `blockEntityAsked` failure:
        # every line present, every grep passing, no row saying anything.
        #
        # So a rule with columns and no admitted cell has to declare it.
        if (v["columns"] and v["gated"] == 0 and v["live"]
                and not e.get("cells_absent")):
            bad.append(("NO-CELLS", f"{cls}.{meth}",
                        f"{v['live']} live cell(s) and the guard admits none, "
                        f"while {v['columns']} says the row is false — either "
                        f"the guard is wrong or the cell was never written, "
                        f"and `cells_absent = true` is how the second is said "
                        f"out loud"))
        if e.get("cells_absent") and v["gated"]:
            bad.append(("NO-CELLS", f"{cls}.{meth}",
                        f"declares `cells_absent` and the guard admits "
                        f"{v['gated']} cell(s)"))
        for callee, hid in sorted(v.get("via", {}).items()):
            if callee not in (e.get("via") or {}):
                bad.append(("VIA", f"{cls}.{meth}",
                            f"calls {callee}, which is itself gated on "
                            f"ServerLevel and hides {hid or ['(nothing mapped)']} "
                            f"— the ledger does not name it, and this rule's "
                            f"own `hides` cannot contain what a second gate "
                            f"one call down decides"))
            elif sorted((e.get("via") or {}).get(callee, {}).get("hides", [])) != hid:
                bad.append(("VIA-HIDES", f"{cls}.{meth}",
                            f"ledger says {callee} hides "
                            f"{sorted((e.get('via') or {}).get(callee, {}).get('hides', []))}, "
                            f"the jar says {hid}"))
        for callee in sorted(e.get("via") or {}):
            if callee not in v.get("via", {}):
                bad.append(("VIA-VANISHED", f"{cls}.{meth}",
                            f"the ledger names {callee} as a second gate and "
                            f"the jar no longer has one there"))
        g = e.get("guard")
        why = guard_bad(g)
        if why:
            bad.append(("GUARD-SYNTAX", f"{cls}.{meth}",
                        f"{g!r}: {why} — the grammar is a `&&` conjunction of "
                        f"`no-branch`, `all-states`, `item-any`, "
                        f"`item-unknown`, `<prop><op><value>` or "
                        f"`item=<id>[,<id>]`"))
        elif not v["guarded"] and g != "no-branch":
            bad.append(("GUARD", f"{cls}.{meth}",
                        f"the gate is the body's first branch, so every state "
                        f"reaches it, but the ledger says {g!r} — `no-branch` "
                        f"is the only honest value here"))
        if v["guarded"] and g == "no-branch":
            bad.append(("GUARD", f"{cls}.{meth}",
                        "the jar branches before the gate and the ledger says "
                        "`no-branch` — some states may never reach it and "
                        "stamping them all would be a claim about cells the "
                        "branch has nothing to do with"))
        for t in guard_terms(g or ""):
            if t in GUARD_NEEDS_WHY and not e.get("guard_why"):
                bad.append(("GUARD-WHY", f"{cls}.{meth}",
                            f"`{t}` overrides a real conditional and needs "
                            f"`guard_why` saying which emptiness of this "
                            f"world, or which refusal of this harness, makes "
                            f"it so"))
    swept = probes()
    for (meth, cls) in sorted(ledger):
        if meth not in swept:
            # An optional probe whose table this tree has never built. Its
            # entries are **skipped, not vanished**: nothing was measured, and
            # `VANISHED` means "the jar stopped gating it", which is a claim
            # about the jar. Reporting one as the other is how a tree that
            # simply has fewer tables looks like a jar that changed.
            continue
        if (cls, meth) not in pops:
            bad.append(("VANISHED", f"{cls}.{meth}",
                        f"in {LEDGER.name} but the jar no longer gates it on "
                        f"ServerLevel — a rule that became reachable and a "
                        f"ledger entry that outlived it look identical here"))
    # # THE CROSS-CHECK, AND WHY IT IS THE ARM WORTH HAVING
    #
    # Skipped under `--emit`, and only there. `--emit` runs *before* the
    # dumpers, from `ci/use-golden.sh` and `ci/placed-golden.sh`, so the table
    # it would be comparing against is the one about to be replaced — on a
    # first run that table has no gate column at all and the emit would refuse
    # to produce the file that lets the dumper write one. The ledger-against-jar
    # arms above still run under `--emit`, which is the half that must stop a
    # stale ledger from reaching a dumper.
    #
    # Everything above compares the jar to a ledger, which two files can agree
    # on while the *table* says something else entirely. This compares the
    # bytecode walk to what the dumper actually stamped — two independent
    # routes to one number, one through `javap` and one through a run of the
    # real rule against a real `BlockState`.
    #
    # It is what catches the failure the ledger cannot see: `-Drustmc.gates`
    # unset, a class chain that resolves nothing, a guard that selects no cell.
    # `UseGolden` throws on the extreme case, but a mechanism that stamps *some*
    # cells and quietly stops at the guard would pass its own alarm.
    for name in sorted({probes()[m] for _, m in pops}) if tables else ():
        if not (DATA / name).is_file():
            bad.append(("NO-TABLE", name,
                        "not generated in this tree — the cross-check between "
                        "the bytecode walk and what the dumper stamped is the "
                        "one arm that compares two independent routes, and a "
                        "missing table silently removes it"))
            continue
        doc = json.loads((DATA / name).read_text())
        stamped, blocks = 0, 0
        # **Keyed per table, not per block.** `SignBlock` is two rules over
        # the same 48 blocks now — one on each half of the right-click — and
        # a dict keyed on the block id alone let `useWithoutItem`'s columns
        # decide what `use_item_on.json`'s cells should carry. 1,344 spurious
        # STAMP problems, every one of them this loop's own bookkeeping.
        want_cols = {}
        for (cls, m), v in pops.items():
            if probes()[m] != name:
                continue
            for b in v["block_ids"] or ():
                want_cols[b] = v["columns"]

        def cells_of(st):
            """Every recorded answer of one state, in either table shape.

            `use_item_on.json` prints a `default` plus `except` groups, so the
            objects that carry `gated` are one level further down than in the
            two tables this arm was written against — and reading `states`
            directly found none of them while the summary said 2,793.
            """
            if "default" not in st:
                return [st]
            return [st["default"]] + [g["cell"] for g in st.get("except", [])]

        for row in doc["table"]:
            for st in row["states"]:
                for cell in cells_of(st):
                    if cell.get("gated"):
                        stamped += 1
            if "serverLevelGate" in row:
                blocks += 1
            want = want_cols.get(row["block"])
            if want is None:
                continue
            wrong = None
            for st in row["states"]:
                for cell in cells_of(st):
                    if "refused" in cell:
                        continue
                    got = sorted(cell.get("gated", []))
                    if got and got != want:
                        wrong = (st["from"], got)
                        break
                if wrong:
                    break
            if wrong:
                bad.append(("STAMP", row["block"],
                            f"cell {wrong[0]} carries gated={wrong[1]}, the "
                            f"jar derives {want}"))
        field = doc.get("serverLevelGatedCells")
        if field is None:
            bad.append(("UNSTAMPED", name,
                        "no `serverLevelGatedCells` summary field — the dumper "
                        "predates the gate column, so every row behind an "
                        "`instanceof ServerLevel` is saying nothing again"))
        elif field != stamped:
            bad.append(("STAMP-COUNT", name,
                        f"summary says {field} stamped cell(s), the rows carry "
                        f"{stamped}"))
        # Over `gated` and not `live`: the dumper stamps the cells its guard
        # admits, so comparing against every recorded cell of the rule
        # measures the width of the item axis rather than the stamp.
        derived = sum(v["gated"] or 0 for (c, m), v in pops.items()
                      if probes()[m] == name and not v["announces"]
                      and v["columns"])
        if field is not None and field != derived:
            bad.append(("STAMP-DERIVED", name,
                        f"the dumper stamped {field} cell(s) and the bytecode "
                        f"walk derives {derived} — one of the two has stopped "
                        f"describing the jar"))

    # # TWO TOTALS, AND THE SMALLER ONE IS CHECKED IN EVERY TREE
    #
    # `[meta.totals]` is the two required probes and is reproduced everywhere,
    # so the subtotal cannot rot in a tree that has all three tables.
    # `[meta.totals_full]` includes the optional fifth population and is
    # checked only where it was measured. Neither is a hand-written number
    # that nothing reproduces, which one block covering "whatever this tree
    # happens to have" would have been.
    required = totals(pops, only=set(PROBES))
    for key, want in sorted(meta.get("totals", {}).items()):
        if required.get(key) is not None and required[key] != want:
            bad.append(("TOTAL", key,
                        f"ledger {want} != measured {required[key]} "
                        f"(over {sorted(PROBES)})"))
    t = totals(pops)
    if any(m in probes() for m in OPTIONAL_PROBES):
        for key, want in sorted(meta.get("totals_full", {}).items()):
            if t.get(key) is not None and t[key] != want:
                bad.append(("TOTAL-FULL", key,
                            f"ledger {want} != measured {t[key]} "
                            f"(over {sorted(probes())})"))
    return bad, t


def render(pops, t, where=sys.stdout):
    ledger, _ = load()
    print(f"`instanceof ServerLevel` — {t['rules']} probe rule(s) over "
          f"{t['blocks']} registry blocks", file=where)
    for (cls, meth), v in sorted(pops.items(), key=lambda kv: (kv[1]["announces"],
                                                              kv[0])):
        e = ledger.get((meth, cls), {})
        word = "announces" if v["announces"] else "silent   "
        print(f"  {word}  {cls}.{meth}", file=where)
        print(f"             hides    {', '.join(v['hides']) or '(nothing)'}",
              file=where)
        print(f"             column   "
              f"{', '.join(v['columns']) or f'(none in {probes()[meth]})'}",
              file=where)
        print(f"             rows     {v['rows']} of {v['blocks']} block(s), "
              f"{v['live']} live cell(s), {v['refused']} refused, "
              f"{v['gated']} the guard admits", file=where)
        print(f"             verdict  {e.get('verdict', '?')}  "
              f"guard={e.get('guard', '?')}", file=where)
    print(f"\n  {t['silent_rules']} of {t['rules']} take the branch without a "
          f"word, over {t['silent_blocks']} registry blocks", file=where)
    print(f"  {t['false_cells']} cell(s) over {t['false_blocks']} block(s) "
          f"carry a row that is FALSE in a column its table has — counted over "
          f"the cells each guard admits, not every cell of the rule",
          file=where)


def main():
    # `--emit` is deliberately outside this: it is what `ci/placed-golden.sh`
    # runs *before* producing the only file that would satisfy the check, so
    # requiring it here made a fresh worktree unable to build either table.
    # See the note on `registry`.
    if "--emit" not in sys.argv and not (DATA / "set_placed_by.json").is_file():
        print("server-level-gates: no data/vanilla/set_placed_by.json — run "
              "ci/placed-golden.sh (it does not need one; `--emit` runs "
              "without a table on purpose)", file=sys.stderr)
        return 2
    if not jar().is_file():
        print(f"server-level-gates: no jar at {jar()} — this check needs one "
              f"and cannot guess", file=sys.stderr)
        return 2
    gates, unmapped = derive()
    pops = populations(gates)
    bad, t = check(gates, pops, unmapped, tables="--emit" not in sys.argv)

    if "--emit" in sys.argv:
        for kind, who, detail in bad:
            print(f"{kind}  {who} — {detail}", file=sys.stderr)
        if bad:
            print(f"SERVER-LEVEL-GATES: refusing to emit a table the ledger "
                  f"does not describe ({len(bad)} problem(s))", file=sys.stderr)
            return 1
        where = emit(pops)
        if any(v["blocks"] is None for v in pops.values()):
            print(f"server-level-gates -> {where}  (derived only — no table in "
                  f"this tree yet, so the population arms did not run; "
                  f"`--check` after the dumpers runs all of them)")
        else:
            print(f"server-level-gates -> {where}")
        return 0

    if "--census" in sys.argv:
        for probe, table in sorted(CENSUS.items()):
            gates, blocks, unmapped = census(probe)
            have = (DATA / table).is_file()
            print(f"\n`{probe}` -> {table} "
                  f"({'generated in this tree' if have else 'NOT generated in this tree'})"
                  f" — NOT swept by --check")
            for cls, d in sorted(gates.items()):
                cols = columns_for(probe, d["hides"])
                print(f"  {'announces' if d['announces'] else 'silent   '}  "
                      f"{cls:24s} {len(blocks[cls]):3d} block(s)  "
                      f"hides={', '.join(d['hides']) or '(none mapped)'}"
                      + (f"  -> {', '.join(cols)}" if cols else ""))
                for callee, hid in sorted(d.get("via", {}).items()):
                    print(f"                         via {callee} — itself "
                          f"gated, hides {', '.join(hid) or '(nothing mapped)'}")
            n = sum(len(b) for b in blocks.values())
            loud = sum(1 for d in gates.values() if d["announces"])
            print(f"  {len(gates)} gated rule(s) over {n} registry blocks, "
                  f"{len(gates) - loud} of them silent")
            if unmapped:
                by = sorted({c for c, _ in unmapped})
                print(f"  {len(unmapped)} call(s) over {len(by)} class(es) have "
                      f"no token in EFFECTS — the vocabulary was closed against "
                      f"the two swept probes, and this is the list to work "
                      f"through before {probe} can be swept:")
                for cls, ref in sorted(unmapped)[:12]:
                    print(f"      {cls}.{ref.rsplit('.', 1)[-1]}")
                if len(unmapped) > 12:
                    print(f"      ... and {len(unmapped) - 12} more")
        return 0

    if "--table" in sys.argv:
        print("| rule | blocks | live cells | gated cells | hides | "
              "column it belongs in | verdict |")
        print("|---|---:|---:|---:|---|---|---|")
        ledger, _ = load()
        for (cls, meth), v in sorted(pops.items()):
            e = ledger.get((meth, cls), {})
            print(f"| `{cls}.{meth}` | {v['blocks']} | {v['live']} | "
                  f"{v['gated']} | "
                  f"{', '.join(v['hides'])} | "
                  f"{', '.join(v['columns']) or '—'} | "
                  f"{e.get('verdict', '?')} |")
        return 0

    if "--check" in sys.argv:
        for kind, who, detail in bad:
            print(f"{kind}  {who} — {detail}", file=sys.stderr)
        if bad:
            print(f"SERVER-LEVEL-GATES: {len(bad)} problem(s)", file=sys.stderr)
            return 1
        print(f"SERVER-LEVEL-GATES OK — {t['rules']} gated rule(s), "
              f"{t['silent_rules']} silent over {t['silent_blocks']} blocks, "
              f"{t['false_cells']} cell(s) stamped")
        return 0

    render(pops, t)
    return 0


if __name__ == "__main__":
    sys.exit(main())
