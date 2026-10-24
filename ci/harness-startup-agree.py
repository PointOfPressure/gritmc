#!/usr/bin/env python3
"""harness-startup-agree.py — every corpus harness starts its servers the way the gate does.

    ci/harness-startup-agree.py            # check ci/*.sh, exit 1 on a drift
    ci/harness-startup-agree.py --selftest # each rule shown refusing a constructed drift

# Why (2026-09-24, branch `corpus-parallel`)

`ci/t2-gameplay.sh` is the gate; `ci/t2-gameplay-subset.sh`,
`ci/t2-gameplay-survival.sh` and friends are copies of its start-up sequence,
and the copies have now drifted from it twice in ways that fail quietly:

  1. the gate's `time set` confirmation was fixed on 2026-09-05 to the jar's
     own wording (`Set <clock> to <n> tick(s)`, key
     `commands.time.set.absolute`); the subset copy kept grepping the dead
     `Set the time to`, so every subset run on 26.2 failed after its lock wait;
  2. the gate writes `$OUR_DIR/server.pid` when it boots our server with a
     console fifo; the subset copy did not, and `ci/vanilla-server.sh console`
     refuses to write to a fifo whose owner it cannot prove alive — so every
     game rule sent to our server was DROPPED (found recording 798).

Both are properties of the TEXT of a script, so they are checked on the text,
over every `ci/*.sh`, rather than by remembering to diff two files:

  R1  a `rustmc-server run ... --console "<D>/console.fifo"` must be followed,
      in the same file, by writing `<D>/server.pid`;
  R2  no script may grep the dead `Set the time to` wording;
  R3  a script that sends `time set` to a console must confirm it through
      `commands.time.set.absolute` (derived from the jar, `ci/lang-message-re.py`).
"""
import glob
import os
import re
import shutil
import sys
import tempfile

LAUNCH = re.compile(r'rustmc-server\s+run\b')
CONSOLE = re.compile(r'--console\s+"([^"]+)/console\.fifo"')


def code_lines(text):
    """(lineno, line) for every line that is not a comment."""
    for i, line in enumerate(text.split("\n"), 1):
        if line.lstrip().startswith("#"):
            continue
        yield i, line


def violations(path):
    text = open(path, errors="replace").read()
    lines = list(code_lines(text))
    body = "\n".join(l for _, l in lines)
    out = []
    # R1 — gather each launch command across its `\` continuations.
    for k, (ln, line) in enumerate(lines):
        if not LAUNCH.search(line):
            continue
        cmd, j = line, k
        while cmd.rstrip().endswith("\\") and j + 1 < len(lines):
            j += 1
            cmd += "\n" + lines[j][1]
        m = CONSOLE.search(cmd)
        if not m:
            continue
        d = m.group(1)
        pid = re.compile(r'>\s*"' + re.escape(d) + r'/server\.pid"')
        rest = "\n".join(l for _, l in lines[j + 1:])
        if not pid.search(rest):
            out.append(f"R1 {path}:{ln}: our server is started with a console fifo in {d} "
                       f"and nothing writes {d}/server.pid — ci/vanilla-server.sh console "
                       "will DROP every command sent to it")
    # R2
    for ln, line in lines:
        if "Set the time to" in line:
            out.append(f"R2 {path}:{ln}: greps the dead 'Set the time to' wording; 26.2 prints "
                       "'Set <clock> to <n> tick(s)' (commands.time.set.absolute)")
    # R3
    if re.search(r'console\s+"[^"]+"\s+"time set', body) and "commands.time.set.absolute" not in body:
        ln = next(ln for ln, l in lines if re.search(r'console\s+"[^"]+"\s+"time set', l))
        out.append(f"R3 {path}:{ln}: sends 'time set' and never confirms it through "
                   "commands.time.set.absolute")
    return out


def check(root):
    bad = []
    files = sorted(glob.glob(os.path.join(root, "ci", "*.sh")))
    for f in files:
        bad += violations(f)
    for b in bad:
        print(b, file=sys.stderr)
    print(f"harness-startup-agree: {len(files)} scripts, {len(bad)} drift(s)")
    return 1 if bad else 0


GOOD = '''#!/usr/bin/env bash
./target/release/rustmc-server run --port "$OUR_PORT" \\
    --console "$OUR_DIR/console.fifo" \\
    >"$WORK/rustmc.log" 2>&1 &
RUSTMC_PID=$!
echo "$RUSTMC_PID" > "$OUR_DIR/server.pid"
"$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "time set 6000"
re="$(lang_re commands.time.set.absolute 6000)"
grep -qE "$re" "$VAN_DIR/logs/latest.log"
'''


def selftest():
    cases = [
        ("compliant harness", GOOD, 0),
        ("R1: console fifo, no pid file", GOOD.replace('echo "$RUSTMC_PID" > "$OUR_DIR/server.pid"\n', ""), 1),
        ("R1: pid file written for a DIFFERENT dir", GOOD.replace('"$OUR_DIR/server.pid"', '"$OTHER/server.pid"'), 1),
        ("R1: pid file only in a comment", GOOD.replace('echo "$RUSTMC_PID" > "$OUR_DIR/server.pid"',
                                                         '# echo "$RUSTMC_PID" > "$OUR_DIR/server.pid"'), 1),
        ("R2: the dead time-set wording", GOOD + 'grep -q "Set the time to 6000" x\n', 1),
        ("R3: time set with no derived confirmation",
         GOOD.replace('re="$(lang_re commands.time.set.absolute 6000)"\n', 're=Set\n'), 1),
    ]
    ok = bad = 0
    for label, text, want in cases:
        d = tempfile.mkdtemp(prefix="startup-agree.")
        try:
            os.makedirs(os.path.join(d, "ci"))
            open(os.path.join(d, "ci", "h.sh"), "w").write(text)
            so, se = sys.stdout, sys.stderr
            sys.stdout = sys.stderr = open(os.devnull, "w")
            try:
                rc = check(d)
            finally:
                sys.stdout, sys.stderr = so, se
        finally:
            shutil.rmtree(d, ignore_errors=True)
        if rc == want:
            ok += 1
            print(f"  ok    {label} (exit {rc})")
        else:
            bad += 1
            print(f"  FAIL  {label}: exit {rc}, wanted {want}")
    print(f"harness-startup-agree selftest: {ok} passed, {bad} failed")
    return 0 if bad == 0 else 1


if __name__ == "__main__":
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    sys.exit(selftest() if "--selftest" in sys.argv else check(root))
