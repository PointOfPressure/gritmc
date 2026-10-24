#!/usr/bin/env bash
# corpus-floor-falsify.sh — the gate on the gate.
#
#     ./ci/corpus-floor-falsify.sh                       # replays a saved measurement
#     FLOOR=/path/to/floor.json ./ci/corpus-floor-falsify.sh
#
# A gate that cannot fail is not a gate, and this project has shipped several
# that could not. `ci/corpus-floor.py` is worse than most: it is a mechanism
# for **declaring scenarios unstable**, which is exactly how a real defect gets
# hidden. So every way it can be abused gets an injection here, and the run
# says which ones fired.
#
# It replays a **saved measurement** rather than recording anything, so it costs
# seconds and can run in `ci/t0.sh`. If no measurement is on disk it builds one
# from a synthetic ensemble it writes itself — two hand-made projection files —
# so the falsification does not depend on six recordings surviving a disk
# failure. It says which route it took.
#
# THE CASES, AND WHAT EACH ONE IS FOR
#
#   1  the committed declaration                     -> exit 0
#   2  a declared entry deleted                      -> NEWLY UNSTABLE, exit 1
#   3  a stale name declared at a HIGH rate          -> NO LONGER UNSTABLE, exit 1
#   4  a STABLE scenario hand-declared unstable      -> NO LONGER UNSTABLE, exit 1
#   5  a stable scenario put through `emit`          -> REFUSED, exit 2
#   6  an ours-class entry filed as vanilla's        -> MISCLASSIFIED, exit 1
#   7  a vanilla-class entry filed as a repair       -> MISCLASSIFIED, exit 1
#   8  a name in both tables                         -> BOTH TABLES, exit 1
#   9  an entry with its rate deleted                -> NO POWER TEST, exit 1
#  10  the floor file left stale                     -> FLOOR STALE, exit 1
#  11  the corpus gate must not read the unstable file
#  12  the boundary lens must not be reachable from any gate
#  13  a one-recording ensemble                      -> refuse, exit 2
#  14  an entry pasted from `emit`, unfinished       -> NO MECHANISM, exit 1
#  15  an entry with no `class`                      -> NO MECHANISM, exit 1
#  16  the floor file with its projection removed    -> NO PROJECTION, exit 1
#  17  the floor file under another projection       -> STALE PROJECTION, exit 1
#  18  a measurement naming no projection            -> `update` refuses, exit 2
#  19  the gate's annotation, projection removed     -> NOT QUOTABLE
#  20  the gate's annotation, projection mismatched  -> STALE PROJECTION
#  21  the gate's annotation on a matching stamp     -> quotable, no warning
#  22  a run carrying a pair the ensemble never had  -> VOCABULARY DRIFT
#  23  a run that is a PROPER SUBSET of it           -> no warning at all
#  24  the floor file with its vocabulary removed    -> NO VOCABULARY
#  25  a recording that observed nothing             -> refused, not passed
#  26  an ensemble with no vocabulary                -> `measure` refuses, exit 2
#  27  the floor file's vocabulary drifted           -> VOCABULARY STALE, exit 1
#  28  the reporter's old 3-argument call            -> exit 2; the gate passes 4
#
# **Cases 22-28 close the residual 16-21 left**, and 23 is the one they exist
# for. A projection identity says "same comparator?" and cannot say "same
# recorder?": `proj:` covers `compare.rs`, `scenario.rs` and `trace.rs`, so a
# field the comparator acts on can be absent from the recordings entirely and
# the comparator is then inert on one side of the comparison and live on the
# other. Measured 2026-08-09: `twin-homog-20260809` emits 91 `(kind, field)`
# pairs, a run recorded since emits those 91 plus
# `add_entity.{y_rot, x_rot, y_head_rot, data}`, and the standing floor of 9/88
# was printed beside scores from such runs. The direction is asymmetric — `r1`
# of that same ensemble lacks four `take_item_entity.*` pairs because vanilla
# did not pick the item up, so strict equality would red on normal variation.
# Case 23 is that negative control; without it 22 would pass against a warning
# that never stops.
#
# **Cases 16-21 are the newest and they close a check that could not fail.**
# The floor's staleness test was its *denominator* — 88 against 88 — which is a
# test of how many scenarios exist and not of what the comparison looks at. On
# 2026-08-09 `gameplay-container-throw` changed `compare::project` for the first
# time in the project's life, converting two of the nine names in the standing
# floor, and the gate printed `9/88` and stayed green. Cases 19-21 drive
# `ci/corpus-floor-report.py` — the code `ci/t2-gameplay.sh` actually runs —
# with injected floor files, so the annotation that decides whether a published
# figure may be quoted is exercised in a second rather than only by a
# thirty-five-minute corpus run.
#
# **Cases 4 and 5 are the ones this file exists for.** They are the demonstration
# that the mechanism refuses to mark a scenario the reference reproduced — the
# only property that makes it harder to abuse than the hand-written list it
# replaces. Case 14 is the second: `emit` produces every number and refuses to
# guess the mechanism, and an entry pasted from it unfinished must not stand.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); printf '  PASS  %s\n' "$1"; }
no() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$1"; }

# `expect <case> <wanted-exit> <wanted-text> -- <command...>`
expect() {
    local what="$1" want="$2" text="$3"; shift 4
    local out rc
    out="$("$@" 2>&1)"; rc=$?
    if [ "$rc" != "$want" ]; then
        no "$what — exit $rc, wanted $want"
        printf '%s\n' "$out" | sed 's/^/          /' | tail -6
        return
    fi
    if [ -n "$text" ] && ! printf '%s' "$out" | grep -cF "$text" >/dev/null; then
        no "$what — exit $want but no '$text' in the output"
        printf '%s\n' "$out" | sed 's/^/          /' | tail -6
        return
    fi
    ok "$what"
}

# ---------------------------------------------------------------------------
# A measurement to replay. A real one if there is one, a synthetic one if not.
# ---------------------------------------------------------------------------
FLOORFILE_EARLY="$ROOT/ci/checks/gameplay-floor.toml"
FLOOR="${FLOOR:-$ROOT/oracle-work/corpus-floor/floor.json}"
# **A saved measurement is only used when it is the one the committed floor
# file was written from**, matched on the ensemble id. This runs in `ci/t0.sh`,
# and `oracle-work/` is per-tree working state that nothing cleans: a gate
# directory that ran `ci/corpus-floor.sh` for some other branch a week ago
# would leave a `floor.json` here that disagrees with this branch's committed
# declaration, and case 1 would go red for a branch that has done nothing
# wrong. A false red from leftover state is exactly as expensive as a false
# green — the honest reaction to it is to go and edit the declared list.
#
# Mismatch is not an error, it is a fallback: the synthetic route exercises every
# case, and the run says which one it took.
# **THE ENSEMBLE ID IS HALF OF A MEASUREMENT'S IDENTITY AND THE PROJECTION IS
# THE OTHER HALF** — corrected 2026-08-11, on the branch that rebased the
# container state id.
#
# This test used to match on the ensemble id alone, and that is the same error
# `ci/checks/gameplay-floor.toml` grew its `projection` stamp to stop: the
# recordings and the comparator are two independent inputs, and a floor
# measured over one ensemble under two projections is two measurements. A
# branch that changes `compare::project` and then measures the SAME recordings
# gets a `floor.json` whose ensemble id matches the committed floor file while
# describing something the committed file was not written from — so the guard
# below waved it through, case 1 adjudicated a stale committed floor against a
# fresh measurement, and T0 went red on a branch that had done nothing wrong.
# That is precisely the false red the paragraph above says is as expensive as a
# false green.
#
# The staleness itself is real and is NOT hidden by this: `ci/t2-gameplay.sh`
# prints `STALE PROJECTION` on every corpus run, and `ci/corpus-floor.py`
# reports it as its own diagnostic (case 17 below pins that it fires). What
# this decides is only whether a saved measurement is the right INPUT to case
# 1, and a measurement under another projection is not.
if [ -s "$FLOOR" ] && ! python3 - "$FLOOR" "$FLOORFILE_EARLY" <<'PY'
import json, sys, tomllib
try:
    meas = json.load(open(sys.argv[1]))
    with open(sys.argv[2], "rb") as fh:
        floor = tomllib.load(fh)["floor"]
    # An absent stamp on either side is a mismatch, not a match. Two `None`s
    # compare equal, and "neither says which comparator it used" is the one
    # case that must never be read as "they used the same one".
    a = (meas["ensemble"]["id"], meas["projection"])
    b = (floor["ensemble"], floor["projection"])
except Exception:
    raise SystemExit(0)          # unreadable either side -> mismatch -> synthetic
raise SystemExit(1 if a == b else 0)
PY
then
    echo "== replaying the measurement at $FLOOR"
elif [ -s "$FLOOR" ]; then
    echo "== the saved measurement at $FLOOR is not the one" \
         "ci/checks/gameplay-floor.toml was written from (ensemble AND projection" \
         "must both match); using a synthetic ensemble"
    FLOOR=""
fi

# # THE TWO ROUTES ARE NOT THE SAME INSTRUMENT, AND THE SUMMARY LINE SAID THEY
# # WERE. 2026-08-18.
#
# `oracle-work/` is excluded from `ci/t0-remote.sh`'s rsync, so a gate directory
# keeps whatever the last branch to use it left there, and gate directories are
# reused. Whether this stage replays a real measurement or synthesises one is
# therefore decided by **which gate directory a branch happens to land in**,
# and nothing above this line ever said what that costs.
#
# Measured on pristine `origin/main` (cf443588), rsynced gitless exactly as
# `ci/t0-remote.sh` ships it, with `ci/checks/gameplay-floor.toml` untouched:
#
#     no oracle-work/corpus-floor        39 passed,  0 failed   GREEN
#     a saved measurement present        31 passed,  9 failed   RED
#
# Same 39 case names on both routes. The nine that differ are verdicts, not
# coverage — the synthetic ensemble is built such that they pass. The saved
# measurement used above matches this tree's committed floor file on BOTH
# halves of a measurement's identity, ensemble `coverage92-selfcheck-n6-
# 20260811` and projection `proj:7cefde0fb56d9f3e`, so it is a legitimate input
# by this file's own declared criterion and not a stale one.
#
# What it exposes is real and is not this file's to fix: that measurement
# carries **no `vocabulary` key at all**, so cases 22, 23, 24 and 27 — which
# remove or drift a floor file's vocabulary and expect `NO VOCABULARY` or
# `VOCABULARY STALE` — inject nothing into a file that already has nothing, and
# `ci/corpus-floor.py` returns 0 saying nothing. Those four cases cannot fire on
# a real floor file of that vintage, which is this project's central defect
# class, and the synthetic route hides it because a synthesised floor file
# always has a vocabulary.
#
# The repair HERE is only to stop the two routes reporting as one. A green from
# a tree with no saved measurement is a weaker claim than a green from a tree
# with one, and it now says so in a form `ci/box-skip-audit.py` counts, so it
# cannot go back to being invisible. The nine failures are logged against the
# corpus floor area, which owns `ci/corpus-floor.py`.
if [ -z "$FLOOR" ] || [ ! -s "$FLOOR" ]; then
    . "$ROOT/ci/lib/box-env.sh"
    box_skip "corpus-floor-falsify:replay" \
        "no saved measurement at $ROOT/oracle-work/corpus-floor/floor.json —" \
        "the cases ran against a synthetic ensemble, which is a WEAKER" \
        "instrument: measured on main, the same 39 cases give 39/0" \
        "synthetically and 31/9 against a real measurement. This green does" \
        "not cover what a replay would."
fi
# `synthesise_ensemble <dir>` — two hand-made projection files and the manifest
# over them, written into `<dir>`. A function rather than a straight-line block
# because there are now two callers: the fallback below, when no saved
# measurement matches the committed floor file, and **case 6**, which needs a
# scenario the measurement classes as OURS and can no longer assume the real
# ensemble has one.
synthesise_ensemble() {
    mkdir -p "$1"
    python3 - "$1" <<'PY'
import json, sys, pathlib
w = pathlib.Path(sys.argv[1])
# Four scenarios, chosen to exercise every class the adjudicator can reach.
def obs(kind, step, fields, who="actor"):
    return {"who": who, "step": step, "kind": kind, "fields": fields, "when": None}
runs = {}
for i, tag in enumerate(["a", "b"]):
    runs[f"r{i+1}"] = [
        # reproduces in both runs
        {"name": "steady", "about": "", "present": True,
         "declared": [obs("block_update", 0, {"at": [1, 0, 0]})],
         "strict": [], "at_ms": [10]},
        # differs in a field: vanilla's
        {"name": "content-flap", "about": "", "present": True,
         "declared": [obs("add_entity", 0, {"rest": tag})],
         "strict": [], "at_ms": [10]},
        # differs only in which step it was filed under: ours
        {"name": "boundary-flap", "about": "", "present": True,
         "declared": [obs("block_update", i, {"at": [1, 0, 0]})],
         "strict": [], "at_ms": [380 + i]},
        # present in one run, absent in the other: ours
        {"name": "silent-flap", "about": "", "present": True,
         "declared": ([obs("block_update", 0, {"at": [2, 0, 0]})] if i else []),
         "strict": [], "at_ms": ([12] if i else [])},
    ]
for lens in ("declared", "boundary-blind"):
    for rid, scen in runs.items():
        out = json.loads(json.dumps(scen))
        if lens == "boundary-blind":
            for s in out:
                for o in s["declared"]:
                    o["step"] = None
                    o["when"] = None
        suffix = "declared" if lens == "declared" else "blind"
        (w / f"{rid}-{suffix}.json").write_text(json.dumps(
            {"lens": lens, "trace": rid, "target": "t", "version": "26.2",
             "protocol": 776, "recorder": "synthetic", "scenarios": out}))
# The vocabulary each synthetic recording's decoder would have emitted, derived
# from the observations above rather than written out, so the two cannot drift
# apart as the fixture is edited. Both runs get the same one: `vocabulary`
# arriving *unequal* is what cases 22-27 inject, and a fixture that already
# differed would make those injections no-ops.
vocab = sorted({f"{o['kind']}.{f}" for scen in runs["r1"] for o in scen["declared"] for f in o["fields"]})
for rid in runs:
    (w / f"{rid}-vocab.json").write_text(json.dumps(vocab))
# A synthetic ensemble still has to name a comparator: `cmd_update` refuses a
# measurement that does not, which is case 18, and the synthetic route has to
# be able to reach every case after it. This is not the real identity and is
# not meant to look like one.
json.dump({"id": "synthetic", "projection": "proj:5ynthe71c5ynthe7", "runs": [
    {"id": r, "source": f"{r}.json", "sha256": f"{r}-sha", "tree": "synthetic",
     "recorder_digest": "same", "recorded": "1970-01-01T00:00:00Z",
     "declared": str(w / f"{r}-declared.json"), "blind": str(w / f"{r}-blind.json"),
     "vocabulary": str(w / f"{r}-vocab.json")}
    for r in runs]}, open(w / "ensemble.json", "w"))
PY
    ./ci/corpus-floor.py measure "$1/ensemble.json" --out "$1/floor.json" > /dev/null || {
        echo "corpus-floor-falsify: could not build the synthetic measurement" >&2
        exit 2
    }
}
if [ -z "${FLOOR:-}" ] || [ ! -s "$FLOOR" ]; then
    echo "== no saved measurement; building a synthetic ensemble"
    synthesise_ensemble "$WORK"
    FLOOR="$WORK/floor.json"
fi

DECL="$ROOT/ci/checks/gameplay-unstable.toml"
FLOORFILE="$ROOT/ci/checks/gameplay-floor.toml"
# Only the names this measurement actually saw can be injected against it, so
# the four are read out of the measurement rather than written down here — an
# injection against a name the ensemble never recorded would test nothing and
# report `NOT IN THIS RUN`, which looks like a pass.
#
# One python writing shell assignments, rather than four command
# substitutions. The four-substitution form was written first and mis-parsed:
# bash folded two of them into one assignment and case 1 then died on an unset
# variable. A heredoc has no such ambiguity and there is nothing to get right.
python3 - "$FLOOR" > "$WORK/names.sh" <<'PY'
import json, shlex, sys
d = json.load(open(sys.argv[1]))
c = d["by_class"]
def one(*names):
    for k in names:
        if c[k]:
            return c[k][0]
    return ""
print("FLOORED=(" + " ".join(shlex.quote(n) for n in d["floor_names"]) + ")")
# The stable control is NAMED rather than taken alphabetically, so cases 4 and
# 5 keep testing the same thing as the corpus grows. `break-block` is the right
# one: it is the check whose unexplained `match -> diverge` between two runs of
# an unchanged pair of servers is the reason `ci/t2-gameplay-selfcheck.sh`
# exists at all. If the mechanism ever refuses to refuse, it should refuse to
# refuse on that name first.
print("STABLE_NAME=" + shlex.quote(
    "break-block" if "break-block" in c["stable"] else one("stable")))
print("OURS_NAME=" + shlex.quote(one("boundary", "silent")))
print("VANILLA_NAME=" + shlex.quote(one("reference", "mixed")))
PY
# shellcheck disable=SC1090
. "$WORK/names.sh"

echo "  floor names:   ${FLOORED[*]}"
echo "  a stable one:  ${STABLE_NAME:-<none>}"
echo "  an ours one:   ${OURS_NAME:-<none>}"
echo "  a vanilla one: ${VANILLA_NAME:-<none>}"
echo

# `synthesise_base <floor.json> <out.toml>` — the declaration a measurement
# would have if every name in its floor were correctly filed. Two callers, for
# the same reason `synthesise_ensemble` has two.
synthesise_base() {
    python3 - "$1" "$2" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
out = []
for n in d["floor_names"]:
    m = d["measured"][n]
    table = "harness" if m["class"] in ("boundary", "silent") else "unstable"
    out += [f"[[{table}]]", f'name = "{n}"', f"flaps = {m['flaps']}",
            f"comparisons = {m['pairs']}", f"runs = {m['runs']}",
            f'class = "{m["class"]}"', 'measured = "synthetic"',
            'field = "synthetic"' if table == "unstable" else 'repair = "synthetic"',
            'why = "synthetic"',
            # The evidence stamp is required of every declared entry since
            # 2026-08-18: a rate belongs to the ensemble that produced it. The
            # synthetic ensemble names itself, so the fixture can satisfy it
            # honestly rather than being exempted from it.
            f"[{table}.evidence]", f'ensemble = "{d["ensemble"]["id"]}"', ""]
open(sys.argv[2], "w").write("\n".join(out))
PY
}

# A declaration built to match this measurement exactly. For a synthetic run
# that is not the committed file, so it is generated; for a real run it IS the
# committed file, and case 1 is then the real gate.
# The real route uses the COMMITTED declaration, which is the whole value of
# it: case 1 is then this branch's own `gameplay-unstable.toml` and
# `gameplay-floor.toml` adjudicated against a live measurement of the ensemble
# they were written from. The synthetic route cannot test that, only the
# mechanism.
if [ "$FLOOR" != "$WORK/floor.json" ]; then
    BASE="$DECL"
    BASEFLOOR="$FLOORFILE"
else
    BASE="$WORK/base.toml"
    synthesise_base "$FLOOR" "$BASE"
    BASEFLOOR="$WORK/base-floor.toml"
    ./ci/corpus-floor.py update "$FLOOR" --out "$BASEFLOOR" > /dev/null
fi

# `edit <out> <python-body>` — round-trip the declaration through tomllib,
# apply the injection to the parsed `unstable` and `harness` lists, and write a
# minimal TOML back.
#
# **Not a regex over the file text**, which is what this did first and which
# broke on the committed file: entries carry multi-line `why = """..."""`
# blocks with blank lines in them, so a pattern that deletes "up to the next
# blank line" produced a file tomllib could not parse, and the case scored a
# false PASS on exit 1 for the wrong reason. Parsing also makes the injections
# independent of how the file happens to be laid out, which is the property a
# gate on a gate needs most.
#
# The prose fields are dropped in the round trip and that is deliberate: what
# is being injected against is the arithmetic and the class, and an injection
# that also had to reproduce a paragraph of bytecode notes would be testing the
# wrong thing.
# `$EDIT_BASE` overrides `$BASE` for one call, and it is a separate variable
# rather than a `BASE=... edit ...` prefix because a variable assignment in
# front of a *shell function* persists after the call in bash — every later
# case would then have been injected against case 6's synthetic declaration.
edit() {  # edit <out> <python-body>
    local out="$1"; shift
    python3 - "${EDIT_BASE:-$BASE}" "$out" "$@" <<'PY'
import sys, tomllib
src, dst = sys.argv[1], sys.argv[2]
doc = tomllib.load(open(src, "rb"))
unstable = list(doc.get("unstable", []))
harness = list(doc.get("harness", []))
exec(sys.argv[3])
KEYS = ("flaps", "comparisons", "runs", "class")
out = []
for table, rows in (("unstable", unstable), ("harness", harness)):
    for e in rows:
        out.append(f"[[{table}]]")
        out.append(f'name = "{e["name"]}"')
        for k in KEYS:
            if k in e:
                v = e[k]
                out.append(f'{k} = "{v}"' if isinstance(v, str) else f"{k} = {v}")
        out.append('measured = "injected by ci/corpus-floor-falsify.sh"')
        out.append('field = "injected"' if table == "unstable" else 'repair = "injected"')
        out.append('why = "injected"')
        out.append("")
open(dst, "w").write("\n".join(out))
PY
}

echo "== the cases"

# 1. The committed declaration against the measurement it was written from.
#
# **THIS CASE IS CONDITIONALLY ARMED AND NOTHING USED TO SAY SO.** On the
# synthetic route `$BASE` and `$BASEFLOOR` are fixtures this script wrote, so
# the case tests the adjudicator and says nothing whatever about
# `ci/checks/gameplay-*.toml`. Only in a tree that has run `ci/corpus-floor.sh`
# against the committed ensemble does it also test the committed declarations —
# and there it is the only thing that does. The header worries about leftover
# state producing a false *red*; the other half is that its absence hides a
# true one, which is what happened: `docs/PARITY-ROADMAP.md` §50.5.
expect "1  the committed declaration is green" 0 "" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$BASE" --floor-file "$BASEFLOOR"
if [ "$BASE" = "$DECL" ]; then
    echo "     (case 1 is armed against the COMMITTED declarations here — a saved" \
         "measurement of the committed ensemble is on disk. See PARITY-ROADMAP §50.5" \
         "before treating a red as this branch's.)"
fi

# 2. A real entry deleted. A flap is positive evidence at any N and this
#    direction is never power-tested, so it must fire whatever the ensemble.
edit "$WORK/c2.toml" "
name = ${FLOORED[0]@Q}
unstable[:] = [e for e in unstable if e['name'] != name]
harness[:] = [e for e in harness if e['name'] != name]
"
expect "2  a declared entry deleted -> NEWLY UNSTABLE" 1 "NEWLY UNSTABLE" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c2.toml" --floor-file "$BASEFLOOR"

# 3. A name this measurement did not flap, declared at a rate this N had the
#    power to see. The staleness direction, which the power test must not eat.
edit "$WORK/c3.toml" "
unstable.append({'name': ${STABLE_NAME@Q}, 'flaps': 24, 'comparisons': 25,
                 'runs': 26, 'class': 'reference'})
"
expect "3  a stale name at a HIGH rate -> NO LONGER UNSTABLE" 1 "NO LONGER UNSTABLE" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c3.toml" --floor-file "$BASEFLOOR"

# 4. THE ONE THIS FILE EXISTS FOR. A scenario the reference reproduced, given a
#    hand-written maximal rate to buy itself an exemption. The rate is a
#    declaration and can say anything; the distinct-projection count is a
#    measurement, and for this name it is 1.
edit "$WORK/c4.toml" "
unstable.append({'name': ${STABLE_NAME@Q}, 'flaps': 15, 'comparisons': 15,
                 'runs': 6, 'class': 'reference'})
"
expect "4  a STABLE scenario hand-declared unstable -> refused" 1 "NO LONGER UNSTABLE" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c4.toml" --floor-file "$BASEFLOOR"

# 5. The same scenario put through the promoter. It must refuse to write the
#    entry at all rather than emit one with a zero in it.
expect "5  emit refuses a stable scenario" 2 "REFUSED" -- \
    ./ci/corpus-floor.py emit "$FLOOR" --only "$STABLE_NAME"

# 6. An instability that is OURS, filed as a fact about vanilla. This is the
#    abuse that matters most: it is how a harness bug stops being fixed.
#
#    **It must not become un-runnable because the corpus is healthy.** This
#    scored `FAIL  6  no ours-class scenario in this measurement to inject
#    with` on 2026-08-09, on the branch that had just *repaired* the only
#    `silent` name in the floor — so the strongest case in this file switched
#    itself off as a side effect of good news, which is the same shape as
#    every check-that-cannot-fail catalogued here, arriving from the one
#    direction nobody watches. A measurement with no ours-class name falls
#    back to the synthetic ensemble, which has a `boundary-flap` and a
#    `silent-flap` by construction, and the run says which it used.
C6_FLOOR="$FLOOR"; C6_BASE="$BASE"; C6_FLOORFILE="$BASEFLOOR"; C6_NAME="$OURS_NAME"
if [ -z "$C6_NAME" ]; then
    synthesise_ensemble "$WORK/s6"
    C6_FLOOR="$WORK/s6/floor.json"
    C6_BASE="$WORK/s6/base.toml"
    C6_FLOORFILE="$WORK/s6/base-floor.toml"
    synthesise_base "$C6_FLOOR" "$C6_BASE"
    ./ci/corpus-floor.py update "$C6_FLOOR" --out "$C6_FLOORFILE" > /dev/null
    C6_NAME="$(python3 -c 'import json,sys
c = json.load(open(sys.argv[1]))["by_class"]
print((c["boundary"] or c["silent"] or [""])[0])' "$C6_FLOOR")"
    echo "  6  no ours-class name in this measurement; using the synthetic ensemble ($C6_NAME)"
fi
if [ -n "$C6_NAME" ]; then
    EDIT_BASE="$C6_BASE" ; edit "$WORK/c6.toml" "
name = ${C6_NAME@Q}
moved = [e for e in harness if e['name'] == name]
harness[:] = [e for e in harness if e['name'] != name]
for e in moved:
    e['class'] = 'reference'
unstable.extend(moved)
"
    unset EDIT_BASE
    expect "6  an ours-class entry filed as vanilla's -> MISCLASSIFIED" 1 "MISCLASSIFIED" -- \
        ./ci/corpus-floor.py adjudicate "$C6_FLOOR" --declared "$WORK/c6.toml" \
            --floor-file "$C6_FLOORFILE"
else
    no "6  even the synthetic ensemble produced no ours-class scenario"
fi

# 7. And the other direction: vanilla's own nondeterminism filed as a repair
#    order, which would send somebody to fix a harness that is working.
edit "$WORK/c7.toml" "
name = ${VANILLA_NAME@Q}
moved = [e for e in unstable if e['name'] == name]
unstable[:] = [e for e in unstable if e['name'] != name]
for e in moved:
    e['class'] = 'boundary'
harness.extend(moved)
"
expect "7  a vanilla-class entry filed as a repair -> MISCLASSIFIED" 1 "MISCLASSIFIED" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c7.toml" --floor-file "$BASEFLOOR"

# 8. Both tables at once, which is an unfalsifiable claim rather than a strong
#    one: whatever the measurement says, one of the two entries is right.
edit "$WORK/c8.toml" "
harness.append({'name': ${VANILLA_NAME@Q}, 'flaps': 1, 'comparisons': 15,
                'runs': 6, 'class': 'boundary'})
"
expect "8  a name in both tables -> BOTH TABLES" 1 "BOTH TABLES" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c8.toml" --floor-file "$BASEFLOOR"

# 9. An entry that declares no rate. It must fail closed: an exemption that
#    switches itself on when its own input is missing is the defect class this
#    whole protocol is a catalogue of.
edit "$WORK/c9.toml" "
unstable.append({'name': ${STABLE_NAME@Q}, 'class': 'reference'})
"
expect "9  an entry with no rate -> NO POWER TEST POSSIBLE" 1 "NO POWER TEST POSSIBLE" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c9.toml" --floor-file "$BASEFLOOR"

# 10. The standing floor file left behind by a measurement that moved. A floor
#     is a published figure and `ci/t2-gameplay.sh` prints it beside every
#     corpus score, so a stale one is worse than none.
python3 - "$BASEFLOOR" "$WORK/c10.toml" <<'PY'
import re, sys
t = open(sys.argv[1]).read()
t = re.sub(r"^floor = \d+$", "floor = 0", t, count=1, flags=re.M)
open(sys.argv[2], "w").write(t)
PY
expect "10 a stale floor file -> FLOOR STALE" 1 "FLOOR STALE" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$BASE" --floor-file "$WORK/c10.toml"

# 11. THE STRUCTURAL GUARANTEE. Nothing in the corpus gate may read the
#     declared-unstable file, because the moment it does, an entry can make a
#     scenario stop failing — which is the one thing this protocol must not be.
# `code_mentions <needle> <file...>` — the needle on a line that is not a
# comment. A plain `grep` was written first and scored a false FAIL the moment
# `ci/t2-gameplay.sh` grew a comment explaining that it deliberately does not
# read the file. A guard that fires on its own documentation trains people to
# delete the documentation.
code_mentions() {
    [ "$1" = "--" ] && shift
    local needle="$1"; shift
    grep -Hn -e "$needle" "$@" 2>/dev/null | grep -v ':[0-9]*: *#'
}
# The reporter is in the list because the block it holds used to be a heredoc
# inside `ci/t2-gameplay.sh` and was covered by this case for free. Moving code
# out of a gate must not move it out of the gate's guarantees.
if HIT="$(code_mentions "gameplay-unstable" ci/t2-gameplay.sh ci/corpus-floor-report.py)"; then
    no "11 the corpus gate reads gameplay-unstable.toml — an entry could excuse a failure"
    printf '        %s\n' "$HIT"
else
    ok "11 the corpus gate does not read gameplay-unstable.toml (outside comments)"
fi

# 12. And the diagnostic lens must not be reachable from a gate either. It
#     deletes real discriminating power on purpose; a gate run under it would
#     pass checks that are firing on the wrong step.
if HIT="$(code_mentions -- "--lens" ci/t2-gameplay.sh ci/t2-gameplay-subset.sh ci/handoff.sh ci/corpus-floor-report.py)"; then
    no "12 a gate script passes --lens"
    printf '        %s\n' "$HIT"
else
    ok "12 no gate script passes --lens (outside comments)"
fi

# 13. One recording. It must refuse rather than report a floor of 0, which is
#     the number a single recording always produces and never means.
python3 - "$WORK" <<'PY'
import json, sys, pathlib
w = pathlib.Path(sys.argv[1])
e = json.load(open(w / "ensemble.json")) if (w / "ensemble.json").exists() else None
if e:
    e["runs"] = e["runs"][:1]
    json.dump(e, open(w / "one.json", "w"))
PY
if [ -s "$WORK/one.json" ]; then
    expect "13 a one-recording ensemble -> refused" 2 "need at least 2" -- \
        ./ci/corpus-floor.py measure "$WORK/one.json" --out "$WORK/one-floor.json"
else
    # A real measurement was replayed, so there is no synthetic ensemble to
    # truncate. Exercise the shell front door instead, which refuses earlier.
    expect "13 a one-recording ensemble -> refused" 2 "need at least 2" -- \
        ./ci/corpus-floor.sh /dev/null
fi

# 14. An entry pasted straight out of `emit` and never finished. `emit` fills in
#     every number and deliberately leaves `field` and `why` as TODO, because a
#     mechanism is read out of the pinned jar and no measurement can supply one.
#     Without this clause the promoter would be a one-command way to silence a
#     scenario: everything an entry needs except the part that makes it a claim.
edit "$WORK/c14.toml" "
for e in unstable + harness:
    if e['name'] == ${FLOORED[0]@Q}:
        e['unfinished'] = True
"
python3 - "$WORK/c14.toml" "${FLOORED[0]}" <<'PYX'
import sys
path, name = sys.argv[1], sys.argv[2]
out, hit = [], False
for line in open(path).read().split("\n"):
    if line == f'name = "{name}"':
        hit = True
    if hit and line.startswith(("field = ", "repair = ")):
        line = line.split(" = ")[0] + ' = "TODO — the packet and the field that moves"'
        hit = False
    out.append(line)
open(path, "w").write("\n".join(out))
PYX
expect "14 an entry pasted from emit, unfinished -> NO MECHANISM" 1 "NO MECHANISM" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c14.toml" --floor-file "$BASEFLOOR"

# 15. An entry with no `class` at all, which is how a repair order gets filed as
#     a fact by omission rather than by assertion — the quieter half of cases 6
#     and 7 and the one nobody would notice in review.
python3 - "$BASE" "$WORK/c15.toml" <<'PYX'
import sys, tomllib
doc = tomllib.load(open(sys.argv[1], "rb"))
out = []
for table in ("unstable", "harness"):
    for e in doc.get(table, []):
        out += [f"[[{table}]]", f'name = "{e["name"]}"']
        for k in ("flaps", "comparisons", "runs"):
            if k in e:
                out.append(f"{k} = {e[k]}")
        out += ['measured = "injected"',
                'field = "injected"' if table == "unstable" else 'repair = "injected"',
                'why = "injected"', ""]
open(sys.argv[2], "w").write("\n".join(out))
PYX
expect "15 an entry with no class -> NO MECHANISM" 1 "NO MECHANISM" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c15.toml" --floor-file "$BASEFLOOR"

# 16. THE STAMP REMOVED. A floor file that does not say which comparator
#     measured it is the pre-2026-08-09 shape, and every floor written in that
#     shape was taken under a projection nobody recorded. It is refused rather
#     than excused, because the alternative is a figure whose provenance is
#     "whatever the tree looked like at the time".
grep -v '^projection = ' "$BASEFLOOR" > "$WORK/c16.toml"
expect "16 a floor file with no projection -> NO PROJECTION" 1 "NO PROJECTION" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$BASE" --floor-file "$WORK/c16.toml"

# 17. THE STAMP LEFT BEHIND BY A COMPARATOR CHANGE. This is the live one: the
#     denominator, the ensemble, every evidence row and every arithmetic check
#     in the file still reproduce, and the only thing wrong is that the number
#     counts what a *different* projection could not reproduce.
#
#     The fixture is built by SETTING the field rather than by substituting it,
#     so the case tests what it says whatever the committed file happens to
#     carry. Written the other way first, as a `sed` over `^projection = `, it
#     was a silent no-op against a floor file that had none — and a no-op
#     injection scores whatever the uninjected file scores, which is the shape
#     of a case that cannot fail.
stamp_floor() {  # stamp_floor <src> <dst> <value>
    python3 - "$1" "$2" "$3" <<'PYX'
import re, sys
src, dst, value = sys.argv[1], sys.argv[2], sys.argv[3]
t = [l for l in open(src).read().split("\n") if not l.startswith("projection = ")]
t.insert(t.index("[floor]") + 1, f'projection = "{value}"')
open(dst, "w").write("\n".join(t))
PYX
}
stamp_floor "$BASEFLOOR" "$WORK/c17.toml" "proj:0000000000000000"
expect "17 a floor file under another projection -> STALE PROJECTION" 1 "STALE PROJECTION" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$BASE" --floor-file "$WORK/c17.toml"

# 18. And the writer, not just the reader. A measurement taken by a binary too
#     old to answer `projection-id` must not be written into the standing file
#     with a placeholder — that would put the unattributed figure back, under a
#     field whose whole purpose is to say it is attributed.
python3 - "$FLOOR" "$WORK/c18-floor.json" <<'PYX'
import json, sys
d = json.load(open(sys.argv[1]))
d.pop("projection", None)
json.dump(d, open(sys.argv[2], "w"))
PYX
expect "18 a measurement naming no projection -> update refuses" 2 "REFUSED" -- \
    ./ci/corpus-floor.py update "$WORK/c18-floor.json" --out "$WORK/c18.toml"

# 19-21. THE GATE'S OWN ANNOTATION, driven directly. `ci/t2-gameplay.sh` calls
#     `ci/corpus-floor-report.py` with the floor file, the run's report and the
#     identity read out of the binary it just ran; these three are that call
#     with the third argument and the file injected. Exit status is always 0 —
#     the annotation is a warning by design — so what is asserted is the text,
#     which is the thing a reader acts on.
#
#     Case 21 is the negative control and it is not decoration: 19 and 20 would
#     both pass against a reporter that printed NOT QUOTABLE unconditionally,
#     which is a warning that says nothing.
REPORT_JSON="$WORK/report.json"
python3 - "$BASEFLOOR" "$REPORT_JSON" <<'PYX'
import json, sys, tomllib
with open(sys.argv[1], "rb") as fh:
    n = tomllib.load(fh)["floor"]["scenarios"]
# The denominator matches, so STALE DENOMINATOR cannot fire and the projection
# lines are the only thing under test.
json.dump({"declared": {"total": n, "match": 0}}, open(sys.argv[2], "w"))
PYX
STAMP="proj:1111111111111111"
stamp_floor "$BASEFLOOR" "$WORK/c21.toml" "$STAMP"
# The fourth argument is the run's own recording. Cases 19-21 are about the
# projection, so this one matches the floor file's vocabulary exactly and the
# vocabulary lines stay silent — an injection is only a test of what it names.
# `rec_from_vocab` builds it out of the floor file itself, so it cannot fall
# out of step with a committed vocabulary that changes.
rec_from_vocab() {  # rec_from_vocab <floor.toml> <dst.json> [extra-pair...] [-- drop-pair...]
    python3 - "$@" <<'PYX'
import json, sys, tomllib
src, dst = sys.argv[1], sys.argv[2]
rest = sys.argv[3:]
cut = rest.index("--") if "--" in rest else len(rest)
extra, drop = rest[:cut], set(rest[cut + 1:])
with open(src, "rb") as fh:
    vocab = tomllib.load(fh)["floor"].get("vocabulary", [])
pairs = [p for p in list(vocab) + extra if p not in drop]
obs = []
for p in pairs:
    kind, _, field = p.partition(".")
    obs.append({"who": "actor", "step": 0, "at_ms": 1, "kind": kind, "fields": {field: 1}})
json.dump({"recorder": "falsify", "target": "t", "version": "26.2", "protocol": 776,
           "scenarios": [{"name": "s", "anchor": [0, 0, 0], "spawn": [0, 0, 0], "obs": obs}]},
          open(dst, "w"))
PYX
}
rec_from_vocab "$WORK/c21.toml" "$WORK/rec-match.json"
expect "19 the annotation with the stamp removed -> NOT QUOTABLE" 0 "NOT QUOTABLE" -- \
    ./ci/corpus-floor-report.py "$WORK/c16.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-match.json"
expect "20 the annotation under another projection -> STALE PROJECTION" 0 "STALE PROJECTION" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "proj:0000000000000000" \
    "$WORK/rec-match.json"
expect "21 the annotation on a matching stamp -> quotable" 0 "same comparator as this run" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-match.json"
if ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-match.json" \
        2>&1 | grep -c "NOT QUOTABLE" >/dev/null; then
    no "21 the annotation says NOT QUOTABLE even when the projection matches"
fi

# 22-28. THE VOCABULARY. `docs/PARITY-ROADMAP.md` §15.4.
#
#     A projection identity is necessary and it is not sufficient: it covers
#     `compare.rs`, `scenario.rs` and `trace.rs` and not `client.rs`, so the
#     same comparator over recordings made by two different decoders is two
#     instruments and `proj:` cannot see it. The live case on 2026-08-09 —
#     `twin-homog-20260809` emits 91 `(kind, field)` pairs and a run recorded
#     since emits those 91 plus `add_entity.{y_rot, x_rot, y_head_rot, data}`,
#     so `compare::exempt_item_spawn_yaw` is inert over the ensemble and live
#     over the run, and the floor of 9/88 sat between them looking quotable.
#
#     **Case 23 is the one this block exists for.** The direction is
#     asymmetric and was measured: `r1` of that ensemble legitimately lacks all
#     four `take_item_entity.*` pairs, because vanilla did not pick the item up
#     in that run. A strict-equality check reds on normal variation in what the
#     reference *did*, and a check that cries wolf is switched off inside a
#     week. 22 without 23 would pass against a reporter that warned
#     unconditionally, which is the same shape as case 21.
rec_from_vocab "$WORK/c21.toml" "$WORK/rec-extra.json" "add_entity.y_rot"
expect "22 a run carrying a pair the ensemble never produced -> VOCABULARY DRIFT" 0 \
    "VOCABULARY DRIFT" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-extra.json"
expect "22 and it names the pair" 0 "add_entity.y_rot" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-extra.json"
expect "22 and it withdraws quotability" 0 "NOT QUOTABLE" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-extra.json"

# 23. The legitimate subset. A recording missing one of the ensemble's pairs is
#     the reference varying, not the decoder changing.
DROPPED="$(python3 -c 'import sys,tomllib
with open(sys.argv[1],"rb") as fh: print(tomllib.load(fh)["floor"]["vocabulary"][0])' \
    "$WORK/c21.toml")"
rec_from_vocab "$WORK/c21.toml" "$WORK/rec-subset.json" -- "$DROPPED"
if ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-subset.json" \
        2>&1 | grep -c "VOCABULARY DRIFT\|NOT QUOTABLE" >/dev/null; then
    no "23 a run that is a proper subset of the ensemble -> cried wolf"
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-subset.json" \
        2>&1 | sed 's/^/          /' | tail -4
else
    ok "23 a run that is a proper subset of the ensemble -> no warning ($DROPPED absent)"
fi
expect "23 and the absence is still reported" 0 "absent from this run" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-subset.json"

# 24. The baseline removed. A floor file with no vocabulary cannot be compared
#     against anything, and the annotation must say so rather than fall through
#     to silence — silence here reads as "checked and fine".
python3 - "$WORK/c21.toml" "$WORK/c24.toml" <<'PYX'
import re, sys
t = open(sys.argv[1]).read()
open(sys.argv[2], "w").write(re.sub(r"(?ms)^vocabulary = \[.*?^\]\n", "", t))
PYX
expect "24 a floor file with no vocabulary -> NO VOCABULARY" 0 "NO VOCABULARY" -- \
    ./ci/corpus-floor-report.py "$WORK/c24.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-match.json"
expect "24 and it withdraws quotability" 0 "NOT QUOTABLE" -- \
    ./ci/corpus-floor-report.py "$WORK/c24.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-match.json"

# 25. THE VACUOUS CONFIGURATION, REFUSED RATHER THAN PASSED. The empty set is a
#     subset of every ensemble, so a recording that observed nothing would
#     produce "no drift" — an instrument reporting a clean result because it had
#     stopped looking, which is this project's signature defect, in the check
#     built to catch a version of it.
printf '%s' '{"recorder":"falsify","scenarios":[{"name":"s","obs":[]}]}' > "$WORK/rec-empty.json"
expect "25 a recording with an empty vocabulary -> refused, not passed" 0 \
    "VOCABULARY UNVERIFIED" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-empty.json"
expect "25 and it withdraws quotability" 0 "NOT QUOTABLE" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP" "$WORK/rec-empty.json"
# The same refusals from the module's own front door. The report-not-a-recording
# trap is the one `ci/corpus-floor.sh` already closes for its own inputs — the
# `r*.json` glob picks up `report-N.json` — and it is closed here too because
# this file is now a second place recording paths come from. Two callers, one
# rule; a trap closed in one caller is not closed.
#
# `$REPORT_JSON` above is `{"declared": {...}}`, the shape this gate's
# `report.json` has, which is neither a trace nor an `outcomes` report — so it
# is the *third* refusal and testing the second against it scored a FAIL that
# was the case being wrong, not the code. Both shapes get their own fixture.
printf '%s' '{"outcomes":[{"name":"x","verdict":"match"}]}' > "$WORK/rec-report.json"
expect "25 a comparison report offered as a recording -> refused" 2 "not a recording" -- \
    ./ci/trace_vocabulary.py "$WORK/rec-report.json"
expect "25 arbitrary JSON offered as a recording -> refused" 2 "not an oracle-runner trace" -- \
    ./ci/trace_vocabulary.py "$REPORT_JSON"

# 26. And the writer, not only the reader. An ensemble built by a
#     `ci/corpus-floor.sh` older than §15.4 carries no vocabulary at all, and
#     measuring it would mint a floor file that silently disables case 22
#     forever after. It refuses.
if [ -s "$WORK/ensemble.json" ]; then OLD_ENS="$WORK/ensemble.json"; else
    synthesise_ensemble "$WORK/c26" > /dev/null 2>&1; OLD_ENS="$WORK/c26/ensemble.json"; fi
python3 - "$OLD_ENS" "$WORK/c26-ensemble.json" <<'PYX'
import json, sys
d = json.load(open(sys.argv[1]))
for r in d["runs"]:
    r.pop("vocabulary", None)
json.dump(d, open(sys.argv[2], "w"))
PYX
expect "26 an ensemble carrying no vocabulary -> measure refuses" 2 "carries no vocabulary" -- \
    ./ci/corpus-floor.py measure "$WORK/c26-ensemble.json" --out "$WORK/c26-floor.json"

# 27. The standing file against the measurement, which is the half `adjudicate`
#     owns. A floor file whose vocabulary has drifted from the ensemble it
#     claims to describe is as stale as one whose projection has.
python3 - "$BASEFLOOR" "$WORK/c27.toml" <<'PYX'
import re, sys
t = open(sys.argv[1]).read()
t = re.sub(r"(?ms)^vocabulary = \[.*?^\]\n", 'vocabulary = ["not_a_kind.not_a_field"]\n', t)
open(sys.argv[2], "w").write(t)
PYX
expect "27 a floor file whose vocabulary drifted -> VOCABULARY STALE" 1 "VOCABULARY STALE" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$BASE" --floor-file "$WORK/c27.toml"

# 28. WIRED IN, WHICH IS THE PART A CHECK DIES OF. Two halves: the reporter must
#     refuse three arguments — so a caller left un-updated fails loudly instead
#     of skipping the vocabulary silently — and the gate must actually pass the
#     recording. `code_mentions` keeps the second half off comment lines, for
#     case 11's reason.
expect "28 the reporter refuses its old three-argument form" 2 "" -- \
    ./ci/corpus-floor-report.py "$WORK/c21.toml" "$REPORT_JSON" "$STAMP"
if code_mentions "corpus-floor-report.py" ci/t2-gameplay.sh \
        | grep -c 'corpus-floor-report' >/dev/null; then
    if grep -A3 'corpus-floor-report.py' ci/t2-gameplay.sh | grep -c 'vanilla.json' >/dev/null; then
        ok "28 ci/t2-gameplay.sh passes its own recording to the reporter"
    else
        no "28 ci/t2-gameplay.sh calls the reporter without the recording — the \
vocabulary check would never run in the gate"
    fi
else
    no "28 ci/t2-gameplay.sh no longer calls ci/corpus-floor-report.py at all"
fi

# 29. THE ENSEMBLE STAMP. A rate belongs to the ensemble that produced it —
#     `docs/PARITY.md` 4.1 states the rule for the corpus floor's 9 and 7 —
#     and `ci/checks/gameplay-unstable.toml` held two ensembles' rates in one
#     list with nothing machine-readable saying which was which. Four of its
#     five entries came from an unnamed N = 26 sweep on main 92f1658 against a
#     corpus of 61; the fifth from `twin-homog-20260809`; the standing floor is
#     `coverage92-selfcheck-n6-20260811`. The disagreement that hid there is
#     real: `a-player-that-leaves-the-ground-is-resynced` reads 8/15 in that
#     file and 5/15 in `gameplay-floor.toml`, same N, same 15 pairs.
python3 - "$BASE" "$WORK/c29.toml" <<'PYX'
import sys, tomllib
doc = tomllib.load(open(sys.argv[1], "rb"))
out = []
for table in ("unstable", "harness"):
    for e in doc.get(table, []):
        out += [f"[[{table}]]", f'name = "{e["name"]}"']
        for k in ("flaps", "comparisons", "runs"):
            if k in e:
                out.append(f"{k} = {e[k]}")
        out += [f'class = "{e["class"]}"', 'measured = "injected"',
                'field = "injected"' if table == "unstable" else 'repair = "injected"',
                'why = "injected"', ""]   # and NO [<table>.evidence]
open(sys.argv[2], "w").write("\n".join(out))
PYX
expect "29 a declared entry with no ensemble stamp -> UNSTAMPED" 1 "UNSTAMPED" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c29.toml" --floor-file "$BASEFLOOR"

# 30. AND THE STAMP HAS TO BUY SOMETHING. An entry whose evidence names exactly
#     the recordings in front of us, and which records no `distinct_projections`,
#     switches off EVIDENCE DOES NOT REPRODUCE — the one check that can
#     contradict its rate. That is how the check came to be unreachable on five
#     live entries out of five: not by being deleted, by being guarded on a
#     field nothing supplied. An entry from ANOTHER ensemble owes nothing and
#     must stay green, which is the second half of this case.
python3 - "$BASE" "$FLOOR" "$WORK/c30.toml" "$WORK/c30-other.toml" <<'PYX'
import json, sys, tomllib
doc = tomllib.load(open(sys.argv[1], "rb"))
meas = json.load(open(sys.argv[2]))
shas = [r["sha256"] for r in meas["ensemble"]["runs"]]
def render(recordings):
    out = []
    for table in ("unstable", "harness"):
        for e in doc.get(table, []):
            out += [f"[[{table}]]", f'name = "{e["name"]}"']
            for k in ("flaps", "comparisons", "runs"):
                if k in e:
                    out.append(f"{k} = {e[k]}")
            out += [f'class = "{e["class"]}"', 'measured = "injected"',
                    'field = "injected"' if table == "unstable" else 'repair = "injected"',
                    'why = "injected"',
                    f"[{table}.evidence]", f'ensemble = "{meas["ensemble"]["id"]}"',
                    "recordings = " + json.dumps(recordings), ""]
    return "\n".join(out)
# exactly these recordings, and no distinct_projections
open(sys.argv[3], "w").write(render(shas))
# a different recording set: another sample, nothing owed
open(sys.argv[4], "w").write(render([s + "-elsewhere" for s in shas]))
PYX
expect "30 an entry over THESE recordings with no distinct_projections -> UNVERIFIABLE" \
    1 "UNVERIFIABLE" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c30.toml" --floor-file "$BASEFLOOR"
expect "30 the same entry over ANOTHER ensemble's recordings -> not its business" \
    0 "" -- \
    ./ci/corpus-floor.py adjudicate "$FLOOR" --declared "$WORK/c30-other.toml" --floor-file "$BASEFLOOR"

echo
echo "corpus-floor-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ] || exit 1
