#!/usr/bin/env python3
# rejoin-probe.py — join a running rustmc, walk somewhere, disconnect, and
# print the position the server put us at on join.
#
#     ./ci/rejoin-probe.py 127.0.0.1 25599 Walker 40.5 --walk-east 6
#
# It prints exactly one machine-readable line, and only one:
#
#     REJOIN-SPAWN x=8.5 y=100.0 z=8.5
#
# WHY IT PRINTS THE JOIN POSITION RATHER THAN CHECKING ANYTHING
#
# The claim under test is "a player who logs out comes back where they left",
# and no single run of a probe can see that: it needs two runs across a server
# restart, and the comparison belongs to whoever ran both.
# `ci/t2-player-survives-a-restart.sh` is that caller. Keeping the assertion
# out of here is what stops this becoming a probe that reports agreement
# between one number and itself.
#
# THE CLIENT IS `ci/dig-probe.py`'s
#
# Imported rather than copied. A second handshake implementation is a second
# thing to get wrong, and the one in `dig-probe` is the one that already
# speaks the whole stack — compression threshold, the configuration phase, the
# teleport acknowledgement.
import importlib.util
import socket
import struct
import sys
import time
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "digprobe", Path(__file__).resolve().parent / "dig-probe.py"
)
dig = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(dig)


def join(host, port, name):
    """Handshake, login, configuration, and the first teleport.

    Returns `(client, (x, y, z))`. Raises on any disconnect, because a
    disconnect here is the gate's answer and swallowing it would turn a dead
    server into a missing line.
    """
    c = dig.C(host, port)
    c.send(
        0,
        dig.vi(776)
        + dig.vi(len("localhost"))
        + b"localhost"
        + struct.pack(">H", port)
        + dig.vi(2),
    )
    nb = name.encode()
    c.send(0, dig.vi(len(nb)) + nb + b"\x00" * 16)
    while True:
        pid, body = c.recv()
        if pid == 3:
            c.thr = dig.rvi(body)[0]
            continue
        if pid == 2:
            break
        if pid == 0:
            raise SystemExit(f"login disconnect: {body[:160]!r}")
    c.send(3, b"")
    lang = b"en_gb"
    c.send(
        0,
        dig.vi(len(lang))
        + lang
        + bytes([10])
        + dig.vi(0)
        + b"\x01"
        + bytes([0x7F])
        + dig.vi(1)
        + b"\x00"
        + b"\x01"
        + dig.vi(0),
    )
    while True:
        pid, body = c.recv()
        if pid == 14:
            c.send(7, dig.vi(0))
        elif pid == 3:
            c.send(3, b"")
            break
        elif pid == 2:
            raise SystemExit(f"config disconnect: {body[:160]!r}")

    spawn = None
    t0 = time.time()
    while time.time() - t0 < 60 and spawn is None:
        pid, body = c.recv()
        if pid == dig.CB_PLAYER_POSITION:
            tp, i = dig.rvi(body)
            x, y, z = struct.unpack(">ddd", body[i : i + 24])
            c.send(dig.SB_ACCEPT_TELEPORT, dig.vi(tp))
            spawn = (x, y, z)
        elif pid == dig.CB_KEEP_ALIVE:
            c.send(dig.SB_KEEP_ALIVE, body)
        elif pid == dig.CB_PING:
            c.send(dig.SB_PONG, body)
        elif pid == dig.CB_DISCONNECT:
            raise SystemExit(f"play disconnect: {body[:160]!r}")
    if spawn is None:
        raise SystemExit("never teleported into the world")
    return c, spawn


def walk_east(c, spawn, steps):
    """Send `steps` position updates a block apart, and return where we ended.

    One block a packet, because the server refuses a jump it thinks is a
    teleport, and settling for a moment after so the tracker has written the
    last one into the entity the save reads from.
    """
    x, y, z = spawn
    for _ in range(steps):
        x += 1.0
        c.send(dig.SB_MOVE_PLAYER_POS, struct.pack(">ddd", x, y, z) + bytes([1]))
        _drain(c, 0.05)
    _drain(c, 1.0)
    return (x, y, z)


def _drain(c, seconds):
    """Answer keep-alives for a moment; ignore everything else."""
    end = time.time() + seconds
    c.s.settimeout(0.1)
    while time.time() < end:
        try:
            pid, body = c.recv()
        except (socket.timeout, EOFError, TimeoutError):
            continue
        if pid == dig.CB_KEEP_ALIVE:
            c.send(dig.SB_KEEP_ALIVE, body)
        elif pid == dig.CB_PING:
            c.send(dig.SB_PONG, body)
        elif pid == dig.CB_DISCONNECT:
            raise SystemExit(f"disconnected mid-walk: {body[:160]!r}")
    c.s.settimeout(10)


def main():
    if len(sys.argv) < 3:
        print(
            "usage: rejoin-probe.py <host> <port> [name] [--walk-east N] [--stay SECONDS]",
            file=sys.stderr,
        )
        return 2
    host, port = sys.argv[1], int(sys.argv[2])
    name = sys.argv[3] if len(sys.argv) > 3 and not sys.argv[3].startswith("--") else "Walker"
    steps = 0
    if "--walk-east" in sys.argv:
        steps = int(sys.argv[sys.argv.index("--walk-east") + 1])

    stay = 0.0
    if "--stay" in sys.argv:
        stay = float(sys.argv[sys.argv.index("--stay") + 1])

    c, spawn = join(host, port, name)
    # The line the gate reads. Printed BEFORE the walk, because it is where
    # the server put us — the walk is what the *next* run has to come back to.
    print(f"REJOIN-SPAWN x={spawn[0]} y={spawn[1]} z={spawn[2]}", flush=True)
    if steps:
        end = walk_east(c, spawn, steps)
        print(f"REJOIN-WALKED x={end[0]} y={end[1]} z={end[2]}", flush=True)

    if stay > 0:
        # `--stay` exists for the ONE case a clean disconnect cannot measure:
        # whether the server saves a player it was never told was leaving.
        # The disconnect path saves; so does the autosave; a probe that always
        # closed its socket could not tell either of those from a shutdown
        # hook, because all three end with the file on disk.
        #
        # So this holds the connection open, answering keep-alives, while the
        # gate signals the process. `STAYING` is flushed so the gate can wait
        # on it rather than on a sleep — a sleep long enough to be safe is
        # long enough to be the thing the gate is really measuring.
        #
        # EOF here is the SUCCESS path, not a failure: the server exiting is
        # what the gate asked for. It is reported as `STAY-EOF` with the
        # elapsed time, so a server that died for some other reason at some
        # other moment is distinguishable from one that shut down when it was
        # told to.
        print(f"STAYING for {stay}s", flush=True)
        began = time.time()
        try:
            _drain(c, stay)
        except SystemExit as e:
            print(f"STAY-DISCONNECT after {time.time() - began:.1f}s: {e}", flush=True)
            return 0
        except (ConnectionResetError, BrokenPipeError, OSError):
            print(f"STAY-EOF after {time.time() - began:.1f}s", flush=True)
            return 0
        print(f"STAY-DONE after {time.time() - began:.1f}s", flush=True)
        return 0

    # A clean close: the play loop sees EOF and runs its disconnect path,
    # which is the one that saves the player. Killing the process instead
    # would leave the socket in the kernel and the save unrun, which is a
    # difference this gate would read as "the save does not work".
    c.s.shutdown(socket.SHUT_RDWR)
    c.s.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
