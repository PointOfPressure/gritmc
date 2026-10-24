#!/usr/bin/env python3
"""bench-probe.py — send one console command to a running Minecraft server and
time how long its JVM stays busy afterwards.

    bench-probe.py <pid> <fifo> <sample> <busy> <idle> <idle-windows> [timeout]

Console commands are read from stdin, one per line, and all of them are
written to the fifo before the first CPU sample is taken — a square larger
than ``forceload``'s 256-chunk cap has to be issued as one command per chunk
row, and the timer has to cover all of them.

Prints one line: ``<wall-seconds> <cpu-seconds> <peak-cores>``.

Why a probe at all, and why this one: the workload is "generate a forceloaded
chunk square", and a chunk only becomes observable from outside the server
when it is saved.  The obvious detector — ``save-all flush`` plus a region
scan on a loop — was the first thing tried and it is wrong, because **Folia
implements neither ``/save-all`` nor ``/tick``**.  Against Folia that probe
measured the five-minute autosave interval and reported 303 s of wall for
33 s of CPU.  A probe one participant silently ignores is not a measurement.

CPU is the one signal all three servers emit identically and none of them can
opt out of.  ``/proc/<pid>/stat`` fields 14 and 15 (utime, stime) cover every
thread in the process, so a worker pool is counted the same as a single
thread.  The square is finished when the process has gone busy and then
stayed under ``idle`` cores for ``idle-windows`` consecutive samples; that
confirmation window is subtracted from the wall time, so the number reported
is the time to the last CPU spent rather than the time to notice.

The sampling loop lives here rather than in the shell because a shell loop
that forks ``awk`` and ``python3`` twice per sample does not sample at the
interval it claims to — it samples at whatever the forks cost, while still
dividing by the nominal interval, which inflates every core figure and delays
the idle edge.
"""

import os
import sys
import time

TICK = os.sysconf("SC_CLK_TCK")


def cpu_seconds(pid: int) -> float:
    with open(f"/proc/{pid}/stat", "rb") as f:
        fields = f.read().rpartition(b")")[2].split()
    # After the ")" that closes comm, field indices shift by 2: utime is the
    # 14th field overall, i.e. index 11 here; stime follows it.
    return (int(fields[11]) + int(fields[12])) / TICK


def main() -> int:
    pid = int(sys.argv[1])
    fifo = sys.argv[2]
    sample = float(sys.argv[3])
    busy_cores = float(sys.argv[4])
    idle_cores = float(sys.argv[5])
    idle_windows = int(sys.argv[6])
    # A square scales as (2r+1)^2, so a timeout that is generous at r=7 is a
    # silent run-killer at r=32: the r=7 table's slowest JVM square was 18.6 s
    # of wall and r=32 is 18.8x the chunks. Tunable, and the default is left
    # at the value the r=7 table was taken with.
    timeout = float(sys.argv[7]) if len(sys.argv) > 7 else 1200.0
    # Settle mode: send nothing, and time how long the server stays busy on
    # its own. That is not idle curiosity — `forceload remove all` drops the
    # tickets on the square just measured and the server then unloads and
    # *writes* it, and at r=32 that is 4,225 chunks of work landing in
    # whatever window comes next. At r=7 it was 225 chunks and invisible.
    # Measured rather than slept through, because a `sleep 3` that is not
    # long enough silently charges one square's save to the next square.
    settle = len(sys.argv) > 8 and sys.argv[8] == "settle"
    commands = [line for line in sys.stdin.read().splitlines() if line.strip()]
    if not commands and not settle:
        print("PROBE no commands on stdin", file=sys.stderr)
        return 5

    c0 = cpu_seconds(pid)
    t0 = time.monotonic()
    # The server holds the fifo open on fd 3 for its whole life, so opening,
    # writing and closing one end here does not EOF its stdin.
    with open(fifo, "w") as f:
        for command in commands:
            f.write(command + "\n")
        f.flush()

    prev_cpu, prev_t = c0, t0
    # In settle mode the server may already be quiet, and waiting for it to go
    # busy first would hang until the 60 s "command ignored?" guard fired.
    busy_seen = settle
    idle_run = 0
    peak = 0.0
    deadline = t0 + timeout
    while time.monotonic() < deadline:
        time.sleep(sample)
        now_t = time.monotonic()
        try:
            now_cpu = cpu_seconds(pid)
        except OSError:
            print("PROBE server exited", file=sys.stderr)
            return 2
        cores = (now_cpu - prev_cpu) / (now_t - prev_t)
        prev_cpu, prev_t = now_cpu, now_t
        peak = max(peak, cores)
        if cores > busy_cores:
            busy_seen = True
            idle_run = 0
        elif busy_seen and cores < idle_cores:
            idle_run += 1
            if idle_run >= idle_windows:
                break
            # A server that never goes busy has ignored the command; say so
            # rather than reporting an impossibly fast square.
        elif not busy_seen and now_t - t0 > 60.0:
            print("PROBE never went busy — command ignored?", file=sys.stderr)
            return 3
        else:
            idle_run = 0
    else:
        print("PROBE timed out", file=sys.stderr)
        return 4

    wall = time.monotonic() - t0 - idle_windows * sample
    print(f"{wall:.2f} {cpu_seconds(pid) - c0:.2f} {peak:.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
