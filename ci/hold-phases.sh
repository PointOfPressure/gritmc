#!/usr/bin/env bash
# The corpus lock's hold, decomposed by phase, on every run that takes it.
#
# WHY THIS EXISTS AND WHY IT IS NOT A ONE-OFF MEASUREMENT.
#
# `docs/CORPUS-LOCK-HOLD.md` decomposed the hold on 2026-08-16 from two
# retrospective instruments — phase mtimes in trees that had run a corpus, and
# twenty-six vanilla `logs/latest.log` files. Both are forensic: they read
# artefacts a run happened to leave behind, they can only see phases that write
# a file, and they cannot see the phases that matter most for a *decision* —
# how long the acquire waited, how long the `cargo build` inside the lock took,
# whether the reference came from the cache.
#
# The third instrument in that document, `ci/corpus-hold-sample.sh`, samples
# from outside. It is correct and it produced no data, because it has to be
# armed at the moment a gate is in stage 6 and the chain was failing earlier.
# **An instrument you have to be present for is an instrument that measures the
# runs you were present for.**
#
# This one is inside the run and costs one `date`, one 24-byte read of
# `/proc/loadavg` and one append per phase — call it 2 ms across a 43-minute
# hold. So it is always on, for every run and every gate on the box, and the N
# accrues without anybody watching.
#
# APPEND SAFETY. Sixty trees write to one file. Every write is a single
# `printf >>` of one short line to a file opened `O_APPEND`, which the kernel
# serialises against the file's offset — two runs cannot interleave a line.
# `ci/hold-phases-falsify.sh` case 7 asserts it with 40 concurrent writers
# rather than taking POSIX's word for it.
#
# NEVER FAILS THE CALLER. A gate must not go red because a measurement could
# not be written; `hold_phase` returns 0 whatever happens. That is the
# unfalsifiable-instrument hazard this repo keeps meeting, so the honesty is
# moved into the *reader*: `report` refuses to decompose a run whose phases are
# not a complete acquire→release sequence, and says which phase is missing.
# A run this file failed to record is a run absent from the report, not a run
# with a plausible-looking wrong number in it.

HOLD_PHASE_DIR="${RUSTMC_HOLD_PHASE_DIR:-$HOME/.rustmc-hold-phases}"
HOLD_PHASE_LOG="$HOLD_PHASE_DIR/phases.tsv"
_HP_OK=""
_HP_RUN=""
_HP_N="-"

# One run id, stable for the life of the process, unique across the box.
# pid alone is not unique over days — pids wrap — so it carries the start
# instant too, which is what separates two runs that happened to get one pid.
hold_phase_init() {
    local checks="${1:-}"
    if mkdir -p "$HOLD_PHASE_DIR" 2>/dev/null && : >>"$HOLD_PHASE_LOG" 2>/dev/null; then
        _HP_OK=1
    else
        echo "hold-phases: cannot write $HOLD_PHASE_LOG — this run will not be measured" >&2
        return 0
    fi
    _HP_RUN="$(hostname -s 2>/dev/null || echo box)-$$-$(date +%s)"
    # The scenario count, taken here rather than read from N_CHECKS, because
    # N_CHECKS is assigned two thirds of the way down `ci/t2-gameplay.sh` and
    # every rate this file exists to compute needs a denominator from the
    # first row onwards. Same expression, so a corpus that grows is followed.
    if [ -n "$checks" ] && [ -d "$checks" ]; then
        # `|| :` and not bare, because every caller of this file runs under
        # `set -euo pipefail`, where a `find | wc -l` that fails takes the gate
        # down with it. An instrument that can turn a green gate red is a worse
        # trade than no instrument, and this is the whole reason
        # `ci/hold-phases-falsify.sh` case 1 sources this file into a
        # `set -euo pipefail` script with an unwritable log directory and
        # asserts the caller still reaches its last line.
        _HP_N=$(find "$checks" -maxdepth 1 -name '*.toml' 2>/dev/null | wc -l) || _HP_N="-"
    fi
    return 0
}

# hold_phase <phase-name> [note]
hold_phase() {
    [ -n "$_HP_OK" ] || return 0
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$(date +%s.%N)" "$(date -Is)" "$_HP_RUN" "${ROOT:-$PWD}" \
        "${RUSTMC_CORPUS_LOCK_ROLE:-run}" "$1" "$_HP_N" \
        "$(cut -d' ' -f1 /proc/loadavg 2>/dev/null || echo -)" \
        "${2:--}" >>"$HOLD_PHASE_LOG" 2>/dev/null || :
    return 0
}

# --- the reader -------------------------------------------------------------
#
# Only reached when this file is executed rather than sourced.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    case "${1:-report}" in
    report|--report)
        python3 - "$HOLD_PHASE_LOG" "${2:-}" <<'PY'
import sys, collections

# WHICH SPANS ARE INSIDE THE HOLD IS READ OUT OF THE DATA, NEVER ASSUMED.
#
# The `cargo build` moved from inside the lock to outside it on 2026-08-18, and
# a reader with the old layout hardcoded would have gone on reporting the build
# as part of the hold — wrong by exactly the amount the change was worth. So
# the hold is `acquire-end .. release`, and a span belongs to it if and only if
# it lies between those two instants. That makes this report the falsifier for
# the move as well as the measurement of it: run it, and the build appears
# under `outside the hold` with the timestamps saying why.
path = sys.argv[1]
want_role = sys.argv[2] if len(sys.argv) > 2 else ""

SPANS = [
    ("build",        "build-begin",    "build-done"),
    ("wait",         "acquire-begin",  "acquire-end"),
    ("ports",        "acquire-end",    "ports-done"),
    ("vanilla-boot", "ports-done",     "vanilla-up"),
    ("reference",    "vanilla-up",     "reference-done"),
    ("vanilla-stop", "reference-done", "vanilla-stopped"),
    ("rustmc-boot",  "vanilla-stopped","rustmc-up"),
    ("subject",      "rustmc-up",      "subject-done"),
    ("compare",      "subject-done",   "compare-done"),
    ("teardown",     "compare-done",   "release"),
]
# On a cache hit no vanilla is booted at all, so three of those markers never
# appear and the served reference sits between the port pick and the subject
# boot. Same instants, different neighbours.
SPANS_HIT = [
    ("build",        "build-begin",    "build-done"),
    ("wait",         "acquire-begin",  "acquire-end"),
    ("ports",        "acquire-end",    "ports-done"),
    ("reference",    "ports-done",     "reference-done"),
    ("rustmc-boot",  "reference-done", "rustmc-up"),
    ("subject",      "rustmc-up",      "subject-done"),
    ("compare",      "subject-done",   "compare-done"),
    ("teardown",     "compare-done",   "release"),
]

# A SHARDED run (`RUSTMC_CORPUS_SHARDS` > 1, 2026-09-24) records every
# reference and subject shard at once, so the per-server spans above do not
# exist for it: there is one vanilla boot per shard, and they overlap each
# other and the subjects. Its shards write nothing to this log; the harness
# writes `shards-begin` (note = the shard count) before launching them and
# `subject-done` when the last one is merged, and that interval is the whole
# of its recording. Reported as its own column rather than squeezed into
# `reference`/`subject`, because splitting one wall-clock interval between two
# overlapping activities would be inventing the split.
SPANS_SHARDED = [
    ("build",        "build-begin",    "build-done"),
    ("wait",         "acquire-begin",  "acquire-end"),
    ("ports",        "acquire-end",    "ports-done"),
    ("shards",       "ports-done",     "subject-done"),
    ("compare",      "subject-done",   "compare-done"),
    ("teardown",     "compare-done",   "release"),
]

runs = collections.OrderedDict()
try:
    fh = open(path)
except OSError:
    print("hold-phases: no log at %s — nothing has been measured yet" % path)
    sys.exit(0)
torn = 0
for line in fh:
    f = line.rstrip("\n").split("\t")
    if len(f) < 9:
        torn += 1
        continue
    t, _iso, run, tree, role, phase, n, load, note = f[:9]
    try:
        t = float(t)
    except ValueError:
        torn += 1
        continue
    d = runs.setdefault(run, {"tree": tree, "role": role, "n": n,
                              "at": {}, "load": []})
    d["at"].setdefault(phase, t)     # first occurrence wins
    try:
        d["load"].append(float(load))
    except ValueError:
        pass

rows, skipped = [], []
for run, d in runs.items():
    at = d["at"]
    if want_role and d["role"] != want_role:
        continue
    if "acquire-end" not in at or "release" not in at:
        skipped.append((d["tree"].split("/")[-1], ",".join(at)))
        continue
    hold_a, hold_b = at["acquire-end"], at["release"]
    hit = "refcache-hit" in at
    inside, outside = {}, {}
    spans = SPANS_SHARDED if "shards-begin" in at else (SPANS_HIT if hit else SPANS)
    for name, a, b in spans:
        if a not in at or b not in at or at[b] < at[a]:
            continue
        # `wait` is neither: it is the queue, which ends where the hold begins.
        # Reported on its own line because it is the cost the lock imposes on
        # everybody else and the hold is the cost it imposes on the box, and
        # adding them would be counting the same minute twice.
        if name == "wait":
            outside["QUEUE WAIT"] = at[b] - at[a]
            continue
        tgt = inside if (at[a] >= hold_a and at[b] <= hold_b) else outside
        tgt[name] = at[b] - at[a]
    rows.append({"tree": d["tree"].split("/")[-1], "role": d["role"], "n": d["n"],
                 "hit": hit, "hold": hold_b - hold_a, "in": inside, "out": outside,
                 "load": max(d["load"]) if d["load"] else 0.0})

if not rows:
    print("hold-phases: %d run(s) recorded, none with a complete "
          "acquire-end..release. Nothing to decompose." % len(runs))
    for tree, have in skipped[-6:]:
        print("  incomplete: %-24s [%s]" % (tree, have))
    sys.exit(0)

order = [n for n, _, _ in SPANS] + ["shards"]
present_in  = [n for n in order if any(n in r["in"]  for r in rows)]
present_out = [n for n in order if any(n in r["out"] for r in rows)]
if any("QUEUE WAIT" in r["out"] for r in rows):
    present_out.append("QUEUE WAIT")

print("corpus lock hold, decomposed — N=%d run(s), %s" % (len(rows), path))
print()
hdr = ("%-22s %-5s %4s %4s %7s " % ("tree", "role", "N", "ref", "hold_m") +
       " ".join("%10s" % n[:10] for n in present_in))
print(hdr); print("-" * len(hdr))
for r in rows:
    cells = [("%10.2f" % (r["in"][n] / 60)) if n in r["in"] else "%10s" % "—"
             for n in present_in]
    print("%-22s %-5s %4s %4s %7.2f " % (r["tree"], r["role"], r["n"],
          "hit" if r["hit"] else "miss", r["hold"] / 60) + " ".join(cells))

def stat(vals):
    return sum(vals) / len(vals), min(vals), max(vals)

print()
holds = [r["hold"] for r in rows]
mh = stat(holds)[0]
print("INSIDE THE HOLD          %3s %8s %7s %7s %7s %7s" %
      ("n", "mean_m", "min", "max", "share", "s/scen"))
for n in present_in:
    vals = [r["in"][n] for r in rows if n in r["in"]]
    m, lo, hi = stat(vals)
    ns = [int(r["n"]) for r in rows if n in r["in"] and r["n"].isdigit()]
    per = ("%7.2f" % (m / (sum(ns) / len(ns)))) if ns and sum(ns) else "%7s" % "—"
    print("  %-22s %3d %8.2f %7.2f %7.2f %6.1f%% %s" %
          (n, len(vals), m / 60, lo / 60, hi / 60, 100 * m / mh, per))
print("  %-22s %3d %8.2f %7.2f %7.2f %6.1f%%" %
      ("HOLD (acq..release)", len(holds), mh / 60, min(holds) / 60,
       max(holds) / 60, 100.0))

if present_out:
    print()
    print("OUTSIDE THE HOLD — measured, and not charged to the box")
    for n in present_out:
        vals = [r["out"][n] for r in rows if n in r["out"]]
        m, lo, hi = stat(vals)
        print("  %-22s %3d %8.2f %7.2f %7.2f" % (n, len(vals), m / 60, lo / 60, hi / 60))

hits = [r for r in rows if r["hit"]]
miss = [r for r in rows if not r["hit"]]
if hits and miss:
    a = stat([r["hold"] for r in hits])[0]
    b = stat([r["hold"] for r in miss])[0]
    print()
    print("reference cache: hold %.2f min on a miss (n=%d), %.2f min on a hit "
          "(n=%d) — %.2fx on the serialised section" %
          (b / 60, len(miss), a / 60, len(hits), b / a))
elif hits:
    a, lo, hi = stat([r["hold"] for r in hits])
    print()
    print("reference cache: %d hit run(s), hold %.2f min (%.2f-%.2f). "
          "No miss run here to compare against." % (len(hits), a / 60, lo / 60, hi / 60))

if torn:
    print()
    print("!! %d malformed line(s) in %s — see ci/hold-phases-falsify.sh case 7"
          % (torn, path))
if skipped:
    print()
    print("%d run(s) not decomposed (no complete acquire-end..release):" % len(skipped))
    for tree, have in skipped[-8:]:
        print("  %-22s [%s]" % (tree, have))
PY
        ;;
    raw)   cat "$HOLD_PHASE_LOG" ;;
    path)  echo "$HOLD_PHASE_LOG" ;;
    *) echo "usage: $0 [report [role]|raw|path]" >&2; exit 2 ;;
    esac
fi
