#!/usr/bin/env python3
"""bench-boot.py — boot time and resident memory, rustmc against vanilla.

    ci/bench-boot.py --flavour rustmc  --runs 10 --level flat   --out r.jsonl
    ci/bench-boot.py --flavour vanilla --runs 10 --level normal --out v.jsonl

One JSON object per run on stdout and appended to `--out`. Nothing is
aggregated here on purpose: the median and the spread are computed by
`ci/bench-boot-report.py` from the raw runs, so a re-reading can check the
aggregation instead of trusting it.

# What "boot" means, and why there are two marks and not one

**Vanilla opens its listening socket before the world exists.** A probe that
timed "the port accepts a TCP connection" would therefore time vanilla's
socket bind and rustmc's *entire spawn-area generation*, and would report a
number flattering to vanilla by a factor of whatever the world costs. The
inverse mistake is equally available: rustmc generates its spawn tile before
`listen()`, so timing to its first accept charges it for work vanilla has
not done yet.

So two marks are taken on every run, from the same `t0`:

  * `t_tcp`  — process start to the first successful TCP connect. This is
               "the socket is open" and on vanilla it means very little.
  * `t_join` — process start to a **real client being in the world**:
               `oracle-runner join`, the same protocol client the gameplay
               oracle drives, returning `JOIN OK` with a spawn position.

`t_join` is the honest headline on both sides, because it is the same event
on both sides: a player can play. `t_tcp` is reported beside it precisely so
the gap is visible rather than hidden.

The join probe is started as soon as the socket accepts, and it is **one
attempt with a long timeout**, not a poll. Vanilla holds a joining client
through spawn preparation rather than refusing it, so a single attempt
completes at the moment the server becomes able to serve — which is the
quantity wanted, at the resolution of the client's own latency rather than
of a retry interval. A refused or reset attempt is retried.

`oracle-runner` is spawned as a subprocess, so every `t_join` on both sides
carries one process spawn. It is the same overhead on both sides and it is
measured by `--probe-overhead` rather than assumed.

# Cold and warm

Three things are cold on a first boot and warm afterwards, and they are not
the same thing:

  * **the page cache** — the jar, the binary, the extracts. `--drop-caches`
    drops it (needs root) and is the only one of the three that can be
    restored to cold without deleting anything;
  * **the vanilla bundler** — `server.jar` is a bundler that extracts
    `versions/` and `libraries/` into its working directory on first run.
    A run directory that is deleted between boots pays that extraction every
    time, which is not what a restarted server pays. So the run directory
    **persists** and only `world/` is deleted, and the extraction cost is
    measured once, separately, as `bundle_extract`;
  * **the world** — a fresh world is generated, an existing one is loaded.
    Different work. `--keep-world` measures the reload; the default measures
    the generation. rustmc only persists when given `--world`, so the reload
    case has to ask for it explicitly on that side.

# Resident memory

`VmRSS` from `/proc/<pid>/status`, sampled every 100 ms for the whole run,
reported at three marks: at `t_join` (idle after boot), the peak up to that
point, and after `--idle-secs` of doing nothing. The third exists to answer
"is it still growing", which a single reading cannot.

**The JVM's RSS is a function of its flags and is not a measurement of need.**
`-Xms4G` touches four gigabytes whether or not anything wants them. So
vanilla is run with `-Xmx` only and no `-Xms`, letting the heap grow to what
the workload actually asks for, and the flag is recorded in every record.
"""

import argparse
import json
import os
import re
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import threading
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def loadavg():
    with open("/proc/loadavg") as f:
        p = f.read().split()
    return [float(p[0]), float(p[1]), float(p[2])]


def rss_kb(pid):
    """Resident set size in kB, or None once the process is gone."""
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1])
    except (FileNotFoundError, ProcessLookupError, PermissionError):
        return None
    return None


def cpu_secs(pid):
    """utime + stime in seconds, or None once the process is gone.

    Wall time on a 24-thread box says more about how many threads a server
    chose to use than about what the work cost, so every wall figure here
    has a CPU figure beside it. Same rule as `ci/bench-servers.sh`.
    """
    try:
        with open(f"/proc/{pid}/stat", "rb") as f:
            fields = f.read().rpartition(b")")[2].split()
        return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK")
    except (FileNotFoundError, ProcessLookupError, IndexError, ValueError):
        return None


def tcp_open(port, timeout=0.05):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout):
            return True
    except OSError:
        return False


class RssSampler(threading.Thread):
    """Sample VmRSS every `interval` seconds until told to stop."""

    def __init__(self, pid, t0, interval=0.1):
        super().__init__(daemon=True)
        self.pid, self.t0, self.interval = pid, t0, interval
        self.samples = []          # (seconds since t0, kB)
        self._stop = threading.Event()

    def run(self):
        while not self._stop.is_set():
            v = rss_kb(self.pid)
            if v is not None:
                self.samples.append((time.monotonic() - self.t0, v))
            self._stop.wait(self.interval)

    def stop(self):
        self._stop.set()

    def at(self, t):
        """RSS at the sample nearest to `t` seconds after t0."""
        if not self.samples:
            return None
        return min(self.samples, key=lambda s: abs(s[0] - t))[1]

    def peak_until(self, t):
        vals = [v for (ts, v) in self.samples if ts <= t]
        return max(vals) if vals else None

    def last(self):
        return self.samples[-1][1] if self.samples else None


def vanilla_jar():
    with open(os.path.join(ROOT, "crates/rustmc-proto/src/lib.rs")) as f:
        ver = re.search(r'GAME_VERSION: &str = "([^"]+)"', f.read()).group(1)
    return ver, os.path.join(ROOT, f"oracle-work/vanilla-{ver}/server.jar")


def write_props(run_dir, port, level, seed, extra, view_distance=4):
    """server.properties, matched to what ci/vanilla-server.sh writes.

    `max-tick-time=-1` is the oracle's own setting and is carried here so a
    boot measured by this script is a boot of the same server the gates
    measure. `spawn-protection=0` and `enforce-secure-profile=false`
    likewise.

    **`view-distance` is a parameter and was once a literal `4`.** That bug
    survived every table in this section without touching one of them,
    because every other experiment pins the view distance to 4 on both sides
    anyway — and then the view-distance-10 sweep asked vanilla for 10, got a
    server still running at 4, and would have compared our 441-chunk client
    against a 117-chunk one. It was caught by `probe_chunks`, which is
    recorded precisely so a configuration that did not take cannot pass as a
    result. Anything that must match on both servers has to be threaded
    through, not written twice.
    """
    ltype = "minecraft:flat" if level == "flat" else "minecraft:normal"
    props = f"""server-port={port}
online-mode=false
level-type={ltype}
level-seed={seed}
view-distance={view_distance}
simulation-distance={view_distance}
spawn-protection=0
enforce-secure-profile=false
sync-chunk-writes=false
max-tick-time=-1
motd=rustmc-bench-boot
"""
    if extra:
        props += extra.rstrip("\n") + "\n"
    with open(os.path.join(run_dir, "server.properties"), "w") as f:
        f.write(props)
    with open(os.path.join(run_dir, "eula.txt"), "w") as f:
        f.write("eula=true\n")


def drop_caches():
    subprocess.run(["sync"], check=False)
    r = subprocess.run(
        ["sudo", "-n", "sh", "-c", "echo 3 > /proc/sys/vm/drop_caches"],
        capture_output=True, text=True,
    )
    return r.returncode == 0


def join_probe(runner, port, name, stay, timeout):
    """One `oracle-runner join`. Returns (ok, stdout)."""
    try:
        r = subprocess.run(
            [runner, "join", f"127.0.0.1:{port}", name, "--stay-secs", str(stay)],
            capture_output=True, text=True, timeout=timeout,
        )
        return (r.returncode == 0 and "JOIN OK" in r.stdout), r.stdout + r.stderr
    except subprocess.TimeoutExpired:
        return False, "TIMEOUT"


def one_run(a, run_dir, port, runner, seq):
    """A single boot. Returns the record dict."""
    rec = {
        "flavour": a.flavour, "level": a.level, "seed": a.seed, "seq": seq,
        "radius": a.radius, "view_distance": a.view_distance,
        "keep_world": bool(a.keep_world), "cold": bool(a.drop_caches),
    }

    if a.flavour == "vanilla":
        ver, jar = vanilla_jar()
        rec["version"] = ver
        rec["heap_flags"] = f"-Xmx{a.heap}"
        world = os.path.join(run_dir, "world")
        if not a.keep_world and os.path.isdir(world):
            shutil.rmtree(world)
        # Bukkit-layout leftovers, harmless on vanilla but cheap to be sure.
        for d in ("world_nether", "world_the_end"):
            p = os.path.join(run_dir, d)
            if not a.keep_world and os.path.isdir(p):
                shutil.rmtree(p)
        write_props(run_dir, port, a.level, a.seed, a.vanilla_props,
                    a.view_distance)
        cmd = ["java", f"-Xmx{a.heap}", "-jar", jar, "--nogui"]
        cwd = run_dir
        logf = open(os.path.join(run_dir, "server.log"), "w")
    else:
        rec["version"] = "rustmc"
        rec["heap_flags"] = None
        cmd = [
            os.path.join(ROOT, "target/release/rustmc-server"), "run",
            "--port", str(port),
            "--view-distance", str(a.view_distance),
        ]
        # `--radius` is passed only when asked for. It used to default to 2
        # here, which meant this harness could not measure the server's OWN
        # shipped default at all: every run pinned the radius to whatever
        # this file said, and a gate written on top of it would keep testing
        # that number after `status::WORLD_RADIUS` moved. `rec["radius"]`
        # below records None for "the server's default", so a reduction can
        # still tell the two apart.
        if a.radius is not None:
            cmd += ["--radius", str(a.radius)]
        if a.level == "flat":
            cmd += ["--level-type", "flat"]
        if a.keep_world:
            # `--autosave-seconds` matters here and the default would make
            # the reload control vacuous. rustmc persists only on its
            # autosave timer (vanilla's 6000 ticks, 300 s); a run that lives
            # for five seconds writes nothing, the next boot finds no world,
            # regenerates it, and the harness reports a "reload" that was
            # really a generation. So the timer is turned down and the run is
            # held open long enough for it to fire — and `saved_chunks` below
            # asserts that it did.
            cmd += ["--world", os.path.join(run_dir, "world"),
                    "--autosave-seconds", str(a.autosave_seconds)]
        cwd = ROOT
        logf = open(os.path.join(run_dir, "server.log"), "w")

    # Pinning wraps the already-built command rather than living inside
    # either flavour's branch above, so both flavours are pinned identically
    # and a diff between them cannot hide in how the wrapper was applied.
    rec["taskset_cpus"] = a.taskset_cpus or None
    if a.taskset_cpus:
        cmd = ["taskset", "-c", a.taskset_cpus] + cmd

    rec["cmd"] = " ".join(cmd)
    if a.drop_caches:
        rec["drop_caches_ok"] = drop_caches()

    rec["load_before"] = loadavg()
    # Wall clock as well as monotonic, so a mark taken here can be aligned
    # against the timestamps in the server's own log. That alignment is what
    # shows vanilla's `Done (Xs)` timer starting several seconds after the
    # process did.
    rec["t0_wall"] = time.time()
    t0 = time.monotonic()
    p = subprocess.Popen(
        cmd, cwd=cwd, stdout=logf, stderr=subprocess.STDOUT,
        stdin=subprocess.PIPE,
    )
    sampler = RssSampler(p.pid, t0)
    sampler.start()

    # --- mark 1: the socket accepts -------------------------------------
    t_tcp = None
    deadline = t0 + a.timeout
    while time.monotonic() < deadline:
        if p.poll() is not None:
            break
        if tcp_open(port):
            t_tcp = time.monotonic() - t0
            break
        time.sleep(0.005)

    # --- mark 2: a real client is in the world ---------------------------
    t_join, join_out, attempts = None, "", 0
    if t_tcp is not None:
        while time.monotonic() < deadline:
            attempts += 1
            ok, out = join_probe(runner, port, "bench", 0,
                                 max(5.0, deadline - time.monotonic()))
            if ok:
                t_join = time.monotonic() - t0
                join_out = out
                break
            join_out = out
            if p.poll() is not None:
                break
            time.sleep(0.05)
    rec["t_tcp_s"] = round(t_tcp, 4) if t_tcp is not None else None
    rec["t_join_s"] = round(t_join, 4) if t_join is not None else None
    rec["join_attempts"] = attempts
    rec["join_out"] = join_out.strip()[:400]

    # RSS at the moment the server became usable, and the peak on the way.
    if t_join is not None:
        rec["rss_at_join_kb"] = sampler.at(t_join)
        rec["rss_peak_to_join_kb"] = sampler.peak_until(t_join)

    # --- the liveness proof: did the world actually arrive? --------------
    # A join that returns a spawn position but no chunks is a server that
    # accepted a player into nothing, and it would read here as a fast boot.
    if t_join is not None and a.chunk_probe:
        ok, out = join_probe(runner, port, "live", a.chunk_probe, 120)
        m = re.search(r"chunks=(\d+) batches=(\d+)", out)
        rec["probe_chunks"] = int(m.group(1)) if m else None
        rec["probe_batches"] = int(m.group(2)) if m else None

    rec["cpu_at_join_s"] = cpu_secs(p.pid)

    # --- optional: generate a defined chunk square, then read RSS --------
    #
    # The two servers reach the same end state by different routes and the
    # difference is stated rather than smoothed over. rustmc has no console,
    # and grows its world a tile at a time as players walk, so the only way
    # to ask it for a defined square is `--radius` at boot — which is what
    # the radius sweep does. Vanilla is driven here, after boot, with
    # `forceload add` on its stdin. Both end up holding (2N+1)^2 chunks; only
    # one of them paid for it before opening its socket, and that is exactly
    # the asymmetry the boot table reports.
    if t_join is not None and a.forceload_radius and a.flavour == "vanilla":
        n = a.forceload_radius
        b1, b2 = -n * 16, n * 16 + 15
        try:
            p.stdin.write(f"forceload add {b1} {b1} {b2} {b2}\n".encode())
            p.stdin.flush()
        except (BrokenPipeError, OSError):
            pass
        # Wait for the generation to finish, measured rather than slept:
        # the process is done when it has gone busy and then stayed under an
        # eighth of a core for three seconds. An unquantified sleep is the
        # shape of mistake ci/bench-servers.sh already made once.
        c0, tq, quiet = cpu_secs(p.pid), time.monotonic(), 0
        busy = False
        while time.monotonic() - tq < a.forceload_timeout and p.poll() is None:
            time.sleep(0.25)
            c1 = cpu_secs(p.pid)
            if c1 is None:
                break
            rate = (c1 - c0) / 0.25
            c0 = c1
            if rate > 0.25:
                busy, quiet = True, 0
            elif busy:
                quiet += 1
                if quiet >= 12:
                    break
        rec["forceload_radius"] = n
        rec["forceload_chunks"] = (2 * n + 1) ** 2
        rec["forceload_wall_s"] = round(time.monotonic() - tq, 2)
        rec["rss_after_gen_kb"] = sampler.last()
        rec["cpu_after_gen_s"] = cpu_secs(p.pid)
    elif t_join is not None and a.forceload_radius:
        # rustmc: the square was generated before the socket opened.
        # `a.radius` is None when the server was left on its own default,
        # and this branch cannot know what that is from out here. Recording
        # null is the honest answer; inventing 2 is what this whole argument
        # stopped doing.
        rec["forceload_radius"] = a.radius
        rec["forceload_chunks"] = (
            None if a.radius is None else (2 * a.radius + 1) ** 2
        )
        rec["rss_after_gen_kb"] = sampler.last()
        rec["cpu_after_gen_s"] = cpu_secs(p.pid)

    # --- optional: N concurrent players ----------------------------------
    #
    # Two things this is meant to catch, both of which are properties of the
    # code rather than guesses:
    #
    #   * `serve_connection` loads the item, placement, shape, use and
    #     setPlacedBy tables **per connection** — they are not behind a
    #     OnceLock — so ten players should mean ten copies;
    #   * `crate::tiles` leaks its `World`s deliberately ("nothing retires a
    #     tile yet"), so anything a player caused to be generated should
    #     still be resident after they leave.
    #
    # Rule 9: a reading of the source moves nothing on its own. So the
    # reading is taken with the players connected AND after they have all
    # gone, and the difference is the measurement.
    if t_join is not None and a.players:
        rec["rss_before_players_kb"] = sampler.last()
        procs = [
            subprocess.Popen(
                [runner, "join", f"127.0.0.1:{port}", f"p{i}",
                 "--stay-secs", str(a.player_secs)],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            for i in range(a.players)
        ]
        time.sleep(max(1.0, a.player_secs * 0.75))
        rec["rss_with_players_kb"] = sampler.last()
        rec["cpu_with_players_s"] = cpu_secs(p.pid)
        for q in procs:
            try:
                q.wait(timeout=180)
            except subprocess.TimeoutExpired:
                q.kill()
        time.sleep(3)
        rec["rss_after_players_kb"] = sampler.last()
        rec["players"] = a.players
        rec["player_secs"] = a.player_secs

    # --- mark 3: RSS after an idle period --------------------------------
    if t_join is not None and a.idle_secs:
        t_mark = time.monotonic()
        rec["rss_idle_start_kb"] = sampler.last()
        rec["cpu_idle_start_s"] = cpu_secs(p.pid)
        while time.monotonic() - t_mark < a.idle_secs and p.poll() is None:
            time.sleep(0.5)
        rec["rss_idle_end_kb"] = sampler.last()
        rec["cpu_idle_end_s"] = cpu_secs(p.pid)
        rec["idle_secs"] = a.idle_secs

    rec["load_after"] = loadavg()

    # --- what the server said about itself, as a cross-check -------------
    sampler.stop()
    try:
        if a.flavour == "vanilla":
            p.stdin.write(b"stop\n")
            p.stdin.flush()
        else:
            p.terminate()
    except (BrokenPipeError, OSError):
        pass
    try:
        p.wait(timeout=60)
    except subprocess.TimeoutExpired:
        p.kill()
        p.wait(timeout=30)
    logf.close()

    with open(os.path.join(run_dir, "server.log"), errors="replace") as f:
        log = f.read()
    if a.flavour == "vanilla":
        m = re.search(r'Done \(([0-9.]+)s\)', log)
        rec["self_reported_s"] = float(m.group(1)) if m else None
        m = re.search(r'Time elapsed: (\d+) ms', log)
        rec["spawn_prep_ms"] = int(m.group(1)) if m else None
        # How long the JVM spent before the server logged anything at all —
        # bundler, class loading, registry and datapack load. It is invisible
        # to `Done (Xs)`, which is why it is taken here from the log's own
        # first timestamp against the wall clock at process start.
        m = re.search(r'\[(\d\d):(\d\d):(\d\d)\]', log)
        if m:
            hh, mm, ss = (int(x) for x in m.groups())
            lt = time.localtime(rec["t0_wall"])
            first = time.mktime((lt.tm_year, lt.tm_mon, lt.tm_mday, hh, mm, ss,
                                 0, 0, lt.tm_isdst))
            # Guard the midnight wrap rather than reporting a negative.
            if first - rec["t0_wall"] < -43200:
                first += 86400
            rec["t_first_log_line_s"] = round(first - rec["t0_wall"], 3)
    else:
        m = re.search(r'spawn tile: (\d+) chunks \((\d+) with the margin\) in ([0-9.]+)s', log)
        if m:
            rec["spawn_chunks"] = int(m.group(1))
            rec["spawn_chunks_with_margin"] = int(m.group(2))
            rec["spawn_prep_ms"] = int(float(m.group(3)) * 1000)
        rec["self_reported_s"] = None
    # The reload control's liveness proof. A "reload" measured against a
    # world that was never written is a generation wearing a different label,
    # and it would read as a fast reload rather than as a broken control.
    if a.keep_world:
        wd = os.path.join(run_dir, "world")
        n = 0
        for _root, _dirs, files in os.walk(wd):
            n += sum(1 for f in files if f.endswith(".mca"))
        rec["world_region_files"] = n
        rec["autosave_writes"] = [int(x) for x in
                                  re.findall(r'autosave: wrote (\d+) chunks', log)]
    rec["exit_code"] = p.returncode

    # The whole RSS trajectory, not just its endpoints. "Is it still
    # growing" is a question about a shape, and two readings cannot tell a
    # flat line from one that rose and fell back to where it started.
    if a.rss_trace:
        path = f"{a.rss_trace}.{a.flavour}.{seq}.csv"
        with open(path, "w") as f:
            f.write("seconds_since_start,rss_kb\n")
            for ts, v in sampler.samples:
                f.write(f"{ts:.2f},{v}\n")
        rec["rss_trace"] = path
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--flavour", required=True, choices=["vanilla", "rustmc"])
    ap.add_argument("--runs", type=int, default=10)
    ap.add_argument("--level", default="flat", choices=["flat", "normal"])
    ap.add_argument("--seed", default="20260803")
    ap.add_argument("--radius", type=int, default=None,
                    help="rustmc tile radius; (2r+1)^2 chunks. "
                         "Omit to use the server's own shipped default.")
    ap.add_argument("--view-distance", type=int, default=4)
    ap.add_argument("--heap", default="1G", help="vanilla -Xmx (no -Xms)")
    ap.add_argument("--port", type=int, default=25911)
    ap.add_argument("--timeout", type=float, default=600)
    ap.add_argument("--idle-secs", type=float, default=0)
    ap.add_argument("--chunk-probe", type=int, default=0,
                    help="after the timed join, join again for N seconds and "
                         "record how many chunks arrived (liveness proof)")
    ap.add_argument("--forceload-radius", type=int, default=0,
                    help="vanilla: forceload a (2N+1)^2 square after boot and "
                         "read RSS once it goes quiet. rustmc: records the "
                         "square its --radius already generated")
    ap.add_argument("--forceload-timeout", type=float, default=900)
    ap.add_argument("--players", type=int, default=0,
                    help="after the timed join, connect N concurrent clients "
                         "and read RSS with them on and after they leave")
    ap.add_argument("--player-secs", type=int, default=12)
    ap.add_argument("--rss-trace", default="",
                    help="write the full RSS sample trajectory to "
                         "<prefix>.<flavour>.<seq>.csv")
    ap.add_argument("--keep-world", action="store_true")
    ap.add_argument("--autosave-seconds", type=int, default=2,
                    help="rustmc --autosave-seconds under --keep-world; the "
                         "default 300 would make the reload control vacuous")
    ap.add_argument("--drop-caches", action="store_true")
    ap.add_argument("--taskset-cpus", default="",
                    help="pin the server process with `taskset -c <cpus>`, "
                         "e.g. '0' for docs/PRODUCT.md's 1-vCPU hardware "
                         "floor. Empty (the default) pins nothing, which is "
                         "how every existing figure in docs/BENCH.md was "
                         "taken — 'anything on one core' is listed there as "
                         "unmeasured because this flag did not exist yet.")
    ap.add_argument("--vanilla-props", default="")
    ap.add_argument("--tag", default="")
    ap.add_argument("--out", required=True)
    ap.add_argument("--work", default=os.path.join(ROOT, "oracle-work/bench-boot"))
    a = ap.parse_args()
    if a.taskset_cpus and not shutil.which("taskset"):
        sys.exit("--taskset-cpus given but `taskset` is not on PATH "
                  "(util-linux)")

    runner = os.path.join(ROOT, "target/release/oracle-runner")
    for b in (runner,) + ((os.path.join(ROOT, "target/release/rustmc-server"),)
                          if a.flavour == "rustmc" else ()):
        if not os.access(b, os.X_OK):
            sys.exit(f"missing binary: {b}")

    run_dir = os.path.join(a.work, f"{a.flavour}-{a.level}{'-' + a.tag if a.tag else ''}")
    os.makedirs(run_dir, exist_ok=True)

    # The vanilla bundler extracts versions/ and libraries/ into its working
    # directory on first run. Measured once here rather than paid — and
    # hidden — inside every timed boot.
    if a.flavour == "vanilla" and not os.path.isdir(os.path.join(run_dir, "libraries")):
        print("# priming the vanilla bundler (extraction is not a boot cost)",
              file=sys.stderr)
        rec = one_run(a, run_dir, a.port, runner, -1)
        rec["bundle_extract"] = True
        rec["tag"] = a.tag
        with open(a.out, "a") as f:
            f.write(json.dumps(rec) + "\n")
        print(json.dumps(rec))

    for i in range(a.runs):
        rec = one_run(a, run_dir, a.port, runner, i)
        rec["bundle_extract"] = False
        rec["tag"] = a.tag
        with open(a.out, "a") as f:
            f.write(json.dumps(rec) + "\n")
        print(json.dumps(rec), flush=True)


if __name__ == "__main__":
    main()
