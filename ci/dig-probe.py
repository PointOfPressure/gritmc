#!/usr/bin/env python3
# dig-probe.py — join a running rustmc, break a block, place one, and check
# the server said so on the wire.
#
#     ./ci/dig-probe.py 127.0.0.1 25599
#
# Why this exists: the unit tests in world_send.rs drive the play loop with a
# fake world, and the tests in decorated_world.rs drive the world with no
# socket. Neither of them can catch a wrong packet id, a frame the server
# never flushes, or a compression threshold applied in one direction only.
# This is the only thing here that speaks the whole stack.
#
# Every id is from Mojang's own report
# (oracle-work/vanilla-<ver>/generated/reports/packets.json), never memory —
# the same rule crates/rustmc-proto/src/ids.rs states. The framing is
# regdump.py's, which is the one that already works against real vanilla.
import json
import os
import socket
import struct
import sys
import time
import zlib

CB_BLOCK_CHANGED_ACK = 4
CB_BLOCK_UPDATE = 8
CB_DISCONNECT = 0x20
CB_KEEP_ALIVE = 0x2C
CB_LEVEL_CHUNK_WITH_LIGHT = 0x2D
CB_LOGIN = 0x31
CB_PING = 0x3D
CB_PLAYER_POSITION = 0x48
SB_ACCEPT_TELEPORT = 0x00
SB_KEEP_ALIVE = 0x1C
SB_PONG = 0x2D
SB_MOVE_PLAYER_POS = 30
SB_MOVE_PLAYER_ROT = 32
SB_PLAYER_ACTION = 41
SB_SET_CARRIED_ITEM = 53
SB_SET_CREATIVE_MODE_SLOT = 56
SB_USE_ITEM_ON = 66

# BlockPos.asLong: x:26 | z:26 | y:12, derived in BlockPos's static
# initialiser. Signed on all three.
def pack_pos(x, y, z):
    return ((x & 0x3FFFFFF) << 38) | ((z & 0x3FFFFFF) << 12) | (y & 0xFFF)


def unpack_pos(v):
    def sext(n, bits):
        n &= (1 << bits) - 1
        return n - (1 << bits) if n >> (bits - 1) else n

    return sext(v >> 38, 26), sext(v, 12), sext(v >> 12, 26)


def vi(n):
    out = b""
    n &= 0xFFFFFFFF
    while True:
        b = n & 0x7F
        n >>= 7
        out += bytes([b | (0x80 if n else 0)])
        if not n:
            return out


def rvi(b, i=0):
    n = sh = 0
    while True:
        x = b[i]
        i += 1
        n |= (x & 0x7F) << sh
        if not x & 0x80:
            break
        sh += 7
    if n >= 1 << 31:
        n -= 1 << 32
    return n, i


class C:
    def __init__(s, h, p):
        s.s = socket.create_connection((h, p), 10)
        s.s.settimeout(10)
        s.thr = -1

    def rd(s, n):
        b = b""
        while len(b) < n:
            c = s.s.recv(n - len(b))
            if not c:
                raise EOFError
            b += c
        return b

    def rvi_sock(s):
        n = sh = 0
        while True:
            x = s.rd(1)[0]
            n |= (x & 0x7F) << sh
            if not x & 0x80:
                return n
            sh += 7

    def send(s, pid, body):
        p = vi(pid) + body
        if s.thr >= 0:
            if len(p) >= s.thr:
                p = vi(len(p)) + zlib.compress(p)
            else:
                p = vi(0) + p
        s.s.sendall(vi(len(p)) + p)

    def recv(s):
        ln = s.rvi_sock()
        raw = s.rd(ln)
        if s.thr >= 0:
            usize, i = rvi(raw)
            raw = zlib.decompress(raw[i:]) if usize else raw[i:]
        pid, i = rvi(raw)
        return pid, raw[i:]


def rstr(b, i):
    n, i = rvi(b, i)
    return b[i : i + n].decode(), i + n


def repo(rel):
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", rel)


def item_id(name):
    with open(repo("data/vanilla/items.json")) as f:
        return json.load(f)["items"][name]


_STATES = {}


def state_name(state_id):
    """Global state id to block name, from the same extract the server uses."""
    if not _STATES:
        with open(repo("data/vanilla/block_props.json")) as f:
            for row in json.load(f)["table"]:
                _STATES[row["id"]] = row["state"]
    return _STATES.get(state_id, f"<unknown state {state_id}>")


def main():
    host, port = sys.argv[1], int(sys.argv[2])
    name = sys.argv[3] if len(sys.argv) > 3 else "Digger"
    c = C(host, port)
    c.send(0, vi(776) + vi(len("localhost")) + b"localhost" + struct.pack(">H", port) + vi(2))
    nb = name.encode()
    c.send(0, vi(len(nb)) + nb + b"\x00" * 16)
    while True:
        pid, body = c.recv()
        if pid == 3:
            c.thr = rvi(body)[0]
            continue
        if pid == 2:
            break
        if pid == 0:
            print("login disconnect:", body[:160])
            return 1
    c.send(3, b"")
    lang = b"en_gb"
    c.send(
        0,
        vi(len(lang)) + lang + bytes([10]) + vi(0) + b"\x01" + bytes([0x7F]) + vi(1) + b"\x00" + b"\x01" + vi(0),
    )
    while True:
        pid, body = c.recv()
        if pid == 14:
            c.send(7, vi(0))
        elif pid == 3:
            c.send(3, b"")
            break
        elif pid == 2:
            print("config disconnect:", body[:160])
            return 1

    # --- play ------------------------------------------------------------
    spawn = None
    game_type = None
    t0 = time.time()
    while time.time() - t0 < 60 and spawn is None:
        pid, body = c.recv()
        if pid == CB_LOGIN:
            game_type = join_game_type(body)
        elif pid == CB_PLAYER_POSITION:
            tp, i = rvi(body)
            x, y, z = struct.unpack(">ddd", body[i : i + 24])
            c.send(SB_ACCEPT_TELEPORT, vi(tp))
            spawn = (x, y, z)
        elif pid == CB_KEEP_ALIVE:
            c.send(SB_KEEP_ALIVE, body)
        elif pid == CB_PING:
            c.send(SB_PONG, body)
        elif pid == CB_DISCONNECT:
            print("play disconnect:", body[:160])
            return 1
    if spawn is None:
        print("FAIL: never teleported into the world")
        return 1
    bx, by, bz = int(spawn[0] // 1), int(spawn[1] // 1), int(spawn[2] // 1)
    print(f"spawned at {spawn}, standing in block ({bx},{by},{bz}); game type {game_type}")

    ok = True
    # --- break -----------------------------------------------------------
    target = (bx, by - 1, bz)
    got = dig(c, target, 1)
    ok &= check("break", got, target, lambda s: s == "minecraft:air", 1)

    # --- place -----------------------------------------------------------
    hold(c, item_id("minecraft:stone"))
    # Right-click the top face of the block below the hole we just made, so
    # the placement lands back in the hole.
    got = use_on(c, (bx, by - 2, bz), FACE_UP, 2)
    ok &= check("place", got, target, lambda s: s.startswith("minecraft:stone"), 2)

    # --- place, with a state the click implies ---------------------------
    # A pillar takes the axis of the face it was clicked on. Clear the
    # neighbour first so there is somewhere for it to go; that break may find
    # air already and produce no update, which is why it is not checked.
    beside = (bx + 1, by - 1, bz)
    dig(c, beside, 3)
    hold(c, item_id("minecraft:oak_log"))
    got = use_on(c, target, FACE_EAST, 4)
    ok &= check("axis", got, beside, lambda s: s == "minecraft:oak_log[axis=x]", 4)

    # --- place, with a state only vanilla can have decided ----------------
    #
    # The pair this whole placement table exists for. `StairBlock` sets
    # FACING to `getHorizontalDirection()` and `AbstractFurnaceBlock` to its
    # **opposite**, and the two blocks are indistinguishable in
    # `block_props.json` — so a stair and a furnace placed from **one** yaw
    # must come back facing opposite ways, or the server is applying a
    # generic rule and is backwards for half the blocks that have `facing`.
    #
    # Yaw −90 is east: `Direction.fromYRot` is `floor(yaw / 90 + 0.5) & 3`
    # indexing SOUTH, WEST, NORTH, EAST, and floor(−0.5) & 3 is 3.
    look(c, -90.0, 0.0)
    seq = 5
    for item, want in (
        ("minecraft:oak_stairs", "minecraft:oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]"),
        ("minecraft:furnace", "minecraft:furnace[facing=west,lit=false]"),
    ):
        dig(c, beside, seq)
        seq += 1
        hold(c, item_id(item))
        got = use_on(c, target, FACE_EAST, seq)
        ok &= check(item.split(":")[1], got, beside, lambda s, w=want: s == w, seq)
        seq += 1

    # --- an item that places a block under another name ------------------
    #
    # `minecraft:redstone` places `minecraft:redstone_wire`, and 13 other
    # items are registered under a name that is not their block's. Guessing
    # from the item's own name places nothing for all 14, which looks exactly
    # like "placement is not implemented" — so this is the check that tells
    # the two apart. The state is not asserted, only the block: redstone
    # reads its neighbours and this square has whatever the world put there.
    dig(c, beside, seq)
    seq += 1
    hold(c, item_id("minecraft:redstone"))
    got = use_on(c, target, FACE_EAST, seq)
    ok &= check(
        "redstone",
        got,
        beside,
        lambda s: s.startswith("minecraft:redstone_wire"),
        seq,
    )
    seq += 1

    # --- a block that cannot stand on its own ----------------------------
    #
    # The one that says whether anything *cascades*. Until this, a block a
    # player changed was written and broadcast and that was the end of it: a
    # torch kept hanging on the air where its support used to be, and only a
    # relog put it right, because the client is not the one who decides.
    #
    # Vanilla's path is `Level.setBlock` -> `updateNeighbourShapes` ->
    # `updateShape` -> `Block.updateOrDestroy`, and this is the whole of it
    # seen from outside: put a torch on a block, break the block, and read
    # **two** `block_update`s off the wire — the support, and the torch that
    # could not stand without it.
    ok &= torch_falls(c, target, beside, seq)
    seq += 4

    # --- something the tick loop scheduled fires -------------------------
    #
    # The clock has run at 20 Hz since before the socket opened and nothing
    # was ever scheduled on it. `CoralBlock.getStateForPlacement` schedules
    # one — `60 + random.nextInt(40)` ticks, three to five seconds — and
    # `CoralBlock.tick` turns it into its dead block if it still has no water
    # touching it. So: place a coral in the open air, send nothing at all,
    # and wait for a `block_update` nobody asked for.
    ok &= coral_dies(c, target, beside, seq)
    seq += 4

    # --- walk off the edge of the world ---------------------------------
    #
    # The world used to be one square: past its edge there was nothing, for
    # ever. It grows a tile at a time now, so walking out of the spawn tile
    # has to make chunks the client has never seen arrive — after a pause,
    # because a tile takes seconds to generate on its own thread.
    #
    # Two chunks east is far enough: the view radius is 2, so standing in
    # chunk 2 pulls chunk 4, which the spawn tile (chunks −2..2 at the
    # default radius) does not own.
    ok &= walked_into_new_ground(c, spawn)

    print("PROBE OK" if ok else "PROBE FAILED")
    return 0 if ok else 1


def torch_falls(c, anchor, support, sequence):
    """Stand a torch on a block, break the block, and read both updates.

    `anchor` is a solid block whose east face the support is placed against —
    the same one the stair and furnace checks click — and `support` is where
    that lands. The torch goes on top of `support`, so breaking `support` is
    a change one block **below** the torch, which is the direction
    `BaseTorchBlock.updateShape` guards on.
    """
    torch = (support[0], support[1] + 1, support[2])
    # Clear whatever the last check left, then rebuild the support.
    dig(c, support, sequence)
    sequence += 1
    hold(c, item_id("minecraft:stone"))
    got = use_on(c, anchor, FACE_EAST, sequence)
    if not check("rig", got, support, lambda s: s.startswith("minecraft:stone"), sequence):
        return False
    sequence += 1
    # The torch itself. A standing torch: `minecraft:torch` clicked on a top
    # face is the block the item places, and the state is its default.
    hold(c, item_id("minecraft:torch"))
    got = use_on(c, support, FACE_UP, sequence)
    if not check("torch", got, torch, lambda s: s == "minecraft:torch", sequence):
        return False
    sequence += 1

    # And now the whole point.
    c.send(
        SB_PLAYER_ACTION,
        vi(0) + struct.pack(">q", pack_pos(*support)) + bytes([FACE_UP]) + vi(sequence),
    )
    changed = {}
    ack = None
    t0 = time.time()
    while time.time() - t0 < 5 and (ack is None or len(changed) < 2):
        try:
            pid, body = c.recv()
        except (socket.timeout, EOFError):
            break
        if pid == CB_KEEP_ALIVE:
            c.send(SB_KEEP_ALIVE, body)
        elif pid == CB_PING:
            c.send(SB_PONG, body)
        elif pid == CB_BLOCK_CHANGED_ACK:
            ack = rvi(body)[0]
        elif pid == CB_BLOCK_UPDATE:
            at = unpack_pos(struct.unpack(">q", body[:8])[0])
            changed[at] = state_name(rvi(body, 8)[0])
    if ack != sequence:
        print(f"FAIL cascade: acknowledged {ack}, sent {sequence}")
        return False
    if changed.get(support) != "minecraft:air":
        print(f"FAIL cascade: the support at {support} is {changed.get(support)}")
        return False
    if changed.get(torch) != "minecraft:air":
        print(
            f"FAIL cascade: the torch at {torch} is {changed.get(torch)} — "
            "nothing cascaded, it is still hanging on air"
        )
        return False
    print(f"  cascade:    block_update minecraft:air at {support} took the torch at {torch}, ack {ack}")
    return True


def coral_dies(c, anchor, support, sequence, seconds=12):
    """Place a coral out of water and wait for the tick loop to kill it.

    The only check here that the server did something **on its own clock**:
    after the placement the probe sends nothing, and the `block_update` that
    arrives is one no packet of ours asked for.

    The coral goes on **top** of the support and not in its place, and that
    is not arbitrary. `RUSTMC_TRACE_TICKS=1` on the first run of this said

        placed minecraft:fire_coral_block at (9, 62, 8) ... water true
          Down=dirt Up=air North=water[level=0] South=grass_block
          West=stone East=water[level=0]

    — the seed-0 spawn is a shoreline and that square is in it, so the
    server was right and the probe was standing in the sea. One block up is
    above the water line.
    """
    where = (support[0], support[1] + 1, support[2])
    dig(c, support, sequence)
    sequence += 1
    hold(c, item_id("minecraft:stone"))
    use_on(c, anchor, FACE_EAST, sequence)
    sequence += 1
    hold(c, item_id("minecraft:fire_coral_block"))
    got = use_on(c, support, FACE_UP, sequence)
    if not check(
        "coral", got, where, lambda s: s == "minecraft:fire_coral_block", sequence
    ):
        return False

    t0 = time.time()
    while time.time() - t0 < seconds:
        try:
            pid, body = c.recv()
        except (socket.timeout, EOFError):
            continue
        if pid == CB_KEEP_ALIVE:
            c.send(SB_KEEP_ALIVE, body)
        elif pid == CB_PING:
            c.send(SB_PONG, body)
        elif pid == CB_BLOCK_UPDATE:
            at = unpack_pos(struct.unpack(">q", body[:8])[0])
            name = state_name(rvi(body, 8)[0])
            if at == where and name == "minecraft:dead_fire_coral_block":
                print(f"  tick:       {name} at {at} {time.time() - t0:.1f}s later, unprompted")
                return True
    print(
        f"FAIL tick: the coral at {where} was still alive after {seconds}s — "
        "nothing is running on the clock"
    )
    return False


def walked_into_new_ground(c, spawn, seconds=90):
    """Walk east and wait for chunks outside the spawn tile."""
    x, y, z = spawn
    seen = set()
    t0 = time.time()
    step = 0
    while time.time() - t0 < seconds:
        # Keep walking east until well outside the tile the join sent, then
        # stand still — standing still is the case that fails if the server
        # only offers chunks when the player crosses a border.
        if step < 6:
            x += 16.0
            step += 1
        c.send(SB_MOVE_PLAYER_POS, struct.pack(">ddd", x, y, z) + bytes([1]))
        try:
            pid, body = c.recv()
        except (socket.timeout, EOFError):
            continue
        if pid == CB_KEEP_ALIVE:
            c.send(SB_KEEP_ALIVE, body)
        elif pid == CB_PING:
            c.send(SB_PONG, body)
        elif pid == CB_LEVEL_CHUNK_WITH_LIGHT:
            cx, cz = struct.unpack(">ii", body[:8])
            seen.add((cx, cz))
            # Anything beyond chunk 2 east is past the spawn tile at the
            # default radius of 2.
            if cx > 2:
                took = time.time() - t0
                print(f"  grew: chunk ({cx}, {cz}) arrived {took:.1f}s after walking out")
                return True
    print(f"FAIL grew: no chunk past the spawn tile in {seconds}s; saw {sorted(seen)[:8]}")
    return False


# `Direction.get3DDataValue`, which is the ordinal for all six.
FACE_UP = 1
FACE_EAST = 5


def dig(c, pos, sequence):
    """START_DESTROY_BLOCK, which is what a creative client sends."""
    c.send(
        SB_PLAYER_ACTION,
        vi(0) + struct.pack(">q", pack_pos(*pos)) + bytes([FACE_UP]) + vi(sequence),
    )
    return collect(c, {CB_BLOCK_UPDATE, CB_BLOCK_CHANGED_ACK}, 5)


def look(c, yaw, pitch):
    """`ServerboundMovePlayerPacket.Rot` — yaw, pitch, then the flags byte.

    The server has no other way to know which way the player is facing, and
    `use_item_on` does not carry it: the packet says where the cursor was and
    never which direction the placer was looking. Sent before the placement
    because the play loop handles packets in the order they arrive.
    """
    c.send(SB_MOVE_PLAYER_ROT, struct.pack(">ff", yaw, pitch) + bytes([1]))


def hold(c, item):
    """Put an item in hotbar slot 0 and select it.

    Menu slot 36 is hotbar slot 0 — `InventoryMenu.USE_ROW_SLOT_START`, not
    zero, which is the trap this line exists to keep exercised.
    """
    c.send(SB_SET_CREATIVE_MODE_SLOT, struct.pack(">h", 36) + vi(1) + vi(item) + vi(0) + vi(0))
    c.send(SB_SET_CARRIED_ITEM, struct.pack(">h", 0))


def use_on(c, pos, face, sequence):
    """Right-click the middle of one face of a block."""
    cursor = {FACE_UP: (0.5, 1.0, 0.5), FACE_EAST: (1.0, 0.5, 0.5)}[face]
    c.send(
        SB_USE_ITEM_ON,
        vi(0)
        + struct.pack(">q", pack_pos(*pos))
        + vi(face)
        + struct.pack(">fff", *cursor)
        + b"\x00\x00"
        + vi(sequence),
    )
    return collect(c, {CB_BLOCK_UPDATE, CB_BLOCK_CHANGED_ACK}, 5)


def join_game_type(body):
    """`play/login`'s game type, by walking the packet rather than scanning it.

    Searching for the `00 ff` of `gameType 0, previousGameType -1` finds the
    right byte only for a survival server, which is exactly the case this
    probe is meant to distinguish.
    """
    i = 4 + 1  # entity id (a plain i32 here), hardcore
    n, i = rvi(body, i)
    for _ in range(n):
        _, i = rstr(body, i)
    for _ in range(3):  # max players, view distance, simulation distance
        _, i = rvi(body, i)
    i += 3  # reduced debug info, show death screen, limited crafting
    _, i = rvi(body, i)  # dimension type
    _, i = rstr(body, i)  # dimension
    i += 8  # the hashed seed
    return body[i]


def collect(c, wanted, seconds):
    """Answer the housekeeping and return the packets asked for."""
    out = []
    t0 = time.time()
    while time.time() - t0 < seconds:
        try:
            pid, body = c.recv()
        except (socket.timeout, EOFError):
            break
        if pid == CB_KEEP_ALIVE:
            c.send(SB_KEEP_ALIVE, body)
        elif pid == CB_PING:
            c.send(SB_PONG, body)
        elif pid in wanted:
            out.append((pid, body))
            if {p for p, _ in out} == wanted:
                break
    return out


def check(what, got, pos, state_ok, sequence):
    ids = [p for p, _ in got]
    if CB_BLOCK_CHANGED_ACK not in ids:
        print(f"FAIL {what}: no block_changed_ack — the client would keep its prediction")
        return False
    ack = rvi(dict(got)[CB_BLOCK_CHANGED_ACK])[0]
    if ack != sequence:
        print(f"FAIL {what}: acknowledged sequence {ack}, sent {sequence}")
        return False
    if CB_BLOCK_UPDATE not in ids:
        print(f"FAIL {what}: acknowledged but no block_update at {pos}")
        return False
    body = dict(got)[CB_BLOCK_UPDATE]
    at = unpack_pos(struct.unpack(">q", body[:8])[0])
    name = state_name(rvi(body, 8)[0])
    if at != pos or not state_ok(name):
        print(f"FAIL {what}: block_update says {name} at {at}, wanted {pos}")
        return False
    print(f"  {what}: block_update {name} at {at}, ack {ack}")
    return True


if __name__ == "__main__":
    sys.exit(main())
