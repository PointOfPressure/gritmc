#!/usr/bin/env bash
# order-selfconsistency-falsify.sh — show ci/order-selfconsistency.py failing.
#
# This check answers "is vanilla self-consistent about this order", and the
# two ways it can be wrong are opposite and both quiet:
#
#   * it reports SELF-CONSISTENT because it found nothing to compare. That is
#     the reading a dead proxy gives, it is indistinguishable from real
#     agreement, and every "no subject" arm below demands a REFUSAL for it.
#   * it reports UNMEASURABLE for something it could have measured. Arm 5
#     flips a real verdict and demands a red, so the pin is load-bearing.
#
# Every arm builds its own recordings under a temp directory. NOTHING here
# writes to ~/rustmc*/oracle-work: that is the shared reference cache, it is
# hours of wall time, and a falsifier that mutates it to prove a point has
# cost more than it proved.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1
CHK=ci/order-selfconsistency.py
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/a" "$TMP/b"

fail=0
arm() { printf '  %-58s %s\n' "$1" "$2"; }

# A minimal pair of recordings, written here rather than copied, so the arms
# below are about the checker and not about whatever the cache holds.
mk() {  # mk <dir> <update_attributes digest> <second entity id>
python3 - "$1" "$2" "$3" <<'EOP'
import json, sys, os
d, dig, ent = sys.argv[1], sys.argv[2], int(sys.argv[3])
rec = {"target": "vanilla", "version": "26.2", "protocol": 0, "recorder": "falsify",
       "scenarios": [{"name": "synthetic-two-entities", "anchor": [0, 0, 0],
                      "spawn": [0, 0, 0], "fresh_session": True, "obs": [
    {"who": "actor", "step": 0, "at_ms": 10, "kind": "update_attributes",
     "fields": {"entity": {"$entity": 2}, "rest": dig}},
    {"who": "actor", "step": 0, "at_ms": 20, "kind": "move_entity_heartbeat",
     "fields": {"entity": {"$entity": 2}, "rest": "aa"}},
    {"who": "actor", "step": 0, "at_ms": 20, "kind": "move_entity_heartbeat",
     "fields": {"entity": {"$entity": ent}, "rest": "bb"}}]}]}
os.makedirs(d, exist_ok=True)
json.dump(rec, open(os.path.join(d, "vanilla.json"), "w"))
EOP
}

PAT="$TMP/*/vanilla.json"

# The same recording under a DIFFERENT scenario name. This is what a lane
# adding a corpus check and recording it actually produces, and it is the
# shape that reddened unmodified main: a row the pin has never held appears in
# the population, and a set comparison cannot tell that from a verdict moving.
mkn() {  # mkn <dir> <scenario name> <digest>
python3 - "$1" "$2" "$3" <<'EOP'
import json, sys, os
d, name, dig = sys.argv[1], sys.argv[2], sys.argv[3]
rec = {"target": "vanilla", "version": "26.2", "protocol": 0, "recorder": "falsify",
       "scenarios": [{"name": name, "anchor": [0, 0, 0],
                      "spawn": [0, 0, 0], "fresh_session": True, "obs": [
    {"who": "actor", "step": 0, "at_ms": 10, "kind": "update_attributes",
     "fields": {"entity": {"$entity": 2}, "rest": dig}},
    {"who": "actor", "step": 0, "at_ms": 20, "kind": "move_entity_heartbeat",
     "fields": {"entity": {"$entity": 2}, "rest": "aa"}},
    {"who": "actor", "step": 0, "at_ms": 20, "kind": "move_entity_heartbeat",
     "fields": {"entity": {"$entity": 7}, "rest": "bb"}}]}]}
os.makedirs(d, exist_ok=True)
json.dump(rec, open(os.path.join(d, "vanilla.json"), "w"))
EOP
}

# 1. no recordings at all.
out="$(python3 "$CHK" --recordings "$TMP/nothing/*.json" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "no recordings match the pattern" "REFUSED (3)  OK"
else arm "no recordings match the pattern" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi

# 2. N = 1. One run always agrees with itself, and reporting that as
#    SELF-CONSISTENT is the single most seductive wrong answer here.
mk "$TMP/a" cafe 7
out="$(python3 "$CHK" --recordings "$TMP/a/vanilla.json" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "N = 1 distinct recording" "REFUSED (3)  OK"
else arm "N = 1 distinct recording" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi

# 3. two recordings that are byte-identical still de-duplicate to N = 1.
mk "$TMP/b" cafe 7
out="$(python3 "$CHK" --recordings "$PAT" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "two identical files de-duplicate to N = 1" "REFUSED (3)  OK"
else arm "two identical files de-duplicate to N = 1" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi

# 4. NO SUBJECT: two genuinely different runs, but nothing emits
#    update_attributes. Agreement over an empty set is not agreement.
mk "$TMP/b" beef 7
python3 - "$TMP" <<'EOP'
import json, sys, glob
for f in glob.glob(sys.argv[1] + "/*/vanilla.json"):
    d = json.load(open(f))
    for s in d["scenarios"]:
        s["obs"] = [o for o in s["obs"] if o["kind"] != "update_attributes"]
    json.dump(d, open(f, "w"))
EOP
out="$(python3 "$CHK" --recordings "$PAT" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "no scenario emits update_attributes" "REFUSED (3)  OK"
else arm "no scenario emits update_attributes" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi

# 5. A VERDICT FLIPS. Against the real cache, force the site the pin records
#    as SELF-CONSISTENT to disagree, by handing the checker a golden table
#    that no longer covers the corpus's id sets. That must refuse, not pass.
GOLD=ci/checks/entity-broadcast-order.json
cp "$GOLD" "$TMP/gold.json"
python3 - "$GOLD" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
d["golden"] = {"1": d["golden"]["1"]}
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
out="$(python3 "$CHK" --check 2>/dev/null)"; rc=$?
if [ "$rc" -eq 3 ] && [ -z "$out" ]; then arm "golden walk table no longer covers the corpus" "REFUSED (3)  OK"
else arm "golden walk table no longer covers the corpus" "rc=$rc stdout=${#out}B  FAIL"; fail=1; fi
cp "$TMP/gold.json" "$GOLD"

# 6. NEGATIVE CONTROL: the real corpus, untouched, is green and its verdicts
#    are the pinned ones.
python3 "$CHK" --check > /dev/null 2>&1; rc=$?
if [ "$rc" -eq 0 ]; then arm "NEGATIVE CONTROL: the real corpus, untouched" "green (0)  OK"
else arm "NEGATIVE CONTROL: the real corpus, untouched" "rc=$rc  FAIL"; fail=1; fi

# 7. NEGATIVE CONTROL: the detector is not firing on everything — the
#    synthetic pair with two DIFFERENT digests is measurable and is reported
#    NOT-SELF-CONSISTENT rather than refused.
mk "$TMP/a" cafe 7; mk "$TMP/b" beef 7
out="$(python3 "$CHK" --recordings "$PAT" --json 2>/dev/null)"; rc=$?
if [ "$rc" -eq 0 ] && printf '%s' "$out" | grep -c 'NOT-SELF-CONSISTENT' >/dev/null; then
    arm "NEGATIVE CONTROL: a real disagreement is measured, not refused" "reported  OK"
else arm "NEGATIVE CONTROL: a real disagreement is measured, not refused" "rc=$rc  FAIL"; fail=1; fi

# ===== THE POPULATION ARMS — rewritten 2026-09-06 ===========================
#
# Declaring the population by sha256 fixed WHICH recordings count and not
# whether they exist, and the recordings live in other lanes' trees: nine lane
# directories created and three destroyed in four hours on 2026-09-05, one
# declared recording already off the box the next morning, and `--check`
# exiting 2 on a tree that had touched none of it. The recordings are this
# check's FIXTURE — what a branch can move is the proxy code and the golden
# walk table — so the fixture is committed and `--check` never globs. These
# arms are about that file.
CIROOT="$TMP/ciroot"
rm -rf "$CIROOT"; mkdir -p "$CIROOT"
cp -r ci "$CIROOT/"
SUB="$CIROOT/ci/order-selfconsistency.py"
CORP="$CIROOT/ci/checks/order-selfconsistency-corpus.json"
PINF="$CIROOT/ci/checks/order-selfconsistency.json"
EMPTYHOME="$TMP/emptyhome"; mkdir -p "$EMPTYHOME"
restore() { rm -rf "$CIROOT"; mkdir -p "$CIROOT"; cp -r ci "$CIROOT/"; }

# P1 CONTROL — a ci/-only $ROOT, which is what a gate directory is.
(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check > /dev/null 2>&1); rc=$?
if [ "$rc" -eq 0 ]; then arm "CONTROL: green in a ci/-only \$ROOT" "green (0)  OK"
else arm "CONTROL: green in a ci/-only \$ROOT" "rc=$rc  FAIL"; fail=1; fi

# P2 THE ARM THIS REWRITE EXISTS FOR. No vanilla recording is reachable at all
#    — every lane directory gone, which is the state a fresh box or a swept one
#    is in. The verdicts are unchanged because they never depended on it.
(cd "$CIROOT" && HOME="$EMPTYHOME" python3 ci/order-selfconsistency.py --check > /dev/null 2>&1); rc=$?
if [ "$rc" -eq 0 ]; then arm "no recording is reachable anywhere on the box" "green (0)  OK"
else arm "no recording is reachable anywhere on the box" "rc=$rc  FAIL"; fail=1; fi

# P2b ...AND THE SHIPPED CHECKER EXITED 2 IN EXACTLY THAT STATE. Without this
#     the arm above proves a checker is green, not that anything changed.
OLD="$TMP/oldroot"
rm -rf "$OLD"; mkdir -p "$OLD"
if git -C "$ROOT" show HEAD:ci/order-selfconsistency.py > "$TMP/old.py" 2>/dev/null \
   && git -C "$ROOT" show HEAD:ci/checks/order-selfconsistency.json > "$TMP/oldpin.json" 2>/dev/null; then
    cp -r ci "$OLD/"
    cp "$TMP/old.py" "$OLD/ci/order-selfconsistency.py"
    cp "$TMP/oldpin.json" "$OLD/ci/checks/order-selfconsistency.json"
    (cd "$OLD" && HOME="$EMPTYHOME" python3 ci/order-selfconsistency.py --check > /dev/null 2>&1); rc=$?
    if [ "$rc" -ne 0 ]; then arm "...and the SHIPPED checker could not run there" "rc=$rc  OK"
    else arm "...and the SHIPPED checker could not run there" "green — nothing changed  FAIL"; fail=1; fi
else
    arm "...and the SHIPPED checker could not run there" "no git history  SKIP"
fi

# P3 the corpus is a TRACKED file, so its absence is a broken tree and not a
#    fact about this box. Refuse, never fall back to a glob.
rm -f "$CORP"
(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check > /dev/null 2>&1); rc=$?
if [ "$rc" -eq 3 ]; then arm "the committed corpus is missing" "REFUSED (3)  OK"
else arm "the committed corpus is missing" "rc=$rc  FAIL"; fail=1; fi
restore

# P4 THE CORPUS IS LOAD-BEARING. Drop recordings from it and the measurement
#    moves. Without this the repair could have been "read a constant", which is
#    green forever and worth nothing.
python3 - "$CORP" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
keep = [r for r in d["recordings"] if any(sc["ua"] for sc in r["scenarios"].values())][:2]
d["recordings"] = keep
json.dump(d, open(p, "w"), sort_keys=True, separators=(",", ":"))
import os
pp = os.path.join(os.path.dirname(p), "order-selfconsistency.json")
pin = json.load(open(pp))
# Both files trimmed together, so the population cross-check stays satisfied
# and the only thing left that can move the answer is the corpus CONTENT.
pin["population"]["recordings"] = [{"sha256": r["sha256"], "seen_at": r.get("seen_at", "?")}
                                   for r in keep]
json.dump(pin, open(pp, "w"), indent=2, sort_keys=True)
EOP
(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check > /dev/null 2>&1); rc=$?
if [ "$rc" -eq 1 ]; then arm "recordings removed from the corpus move the answer" "red (1)  OK"
else arm "recordings removed from the corpus move the answer" "rc=$rc — the corpus is inert  FAIL"; fail=1; fi
restore

# P5 the pin and the corpus are two tracked files describing one population.
#    If they disagree neither can be trusted, so neither is used.
python3 - "$PINF" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
d["population"]["recordings"] = d["population"]["recordings"][:3]
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check > /dev/null 2>&1); rc=$?
if [ "$rc" -eq 3 ]; then arm "pin and corpus declare different populations" "REFUSED (3)  OK"
else arm "pin and corpus declare different populations" "rc=$rc  FAIL"; fail=1; fi
restore

# P6 the reduction dropped every observation whose (who, at_ms) bucket held one
#    row. That is lossless for THAT key and for no other, so a moved key must
#    refuse rather than measure a corpus reduced under the old one.
python3 - "$CORP" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p)); d["bucket_key"] = "who,at_ms,step"
json.dump(d, open(p, "w"), sort_keys=True, separators=(",", ":"))
EOP
(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check > /dev/null 2>&1); rc=$?
if [ "$rc" -eq 3 ]; then arm "the corpus was reduced under a different bucket key" "REFUSED (3)  OK"
else arm "the corpus was reduced under a different bucket key" "rc=$rc  FAIL"; fail=1; fi
restore

# P7 --verify-extract is what makes the committed file evidence and not an
#    assertion. Fabricate a row and it must say so.
python3 - "$CORP" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
# SEARCH for a row rather than assuming where one is. The first recording
# sorted by digest carries no scenario stream at all — a run of vanilla none of
# whose observations either proxy reads — so reaching into `recordings[0]`
# mutated an empty dict and this case passed by fabricating nothing.
done = False
for rec in d["recordings"]:
    for name, sc in sorted(rec["scenarios"].items()):
        if sc["ua"]:
            sc["ua"][0][1] = "0123456789abcdef"
            done = True
            break
    if done:
        break
if not done:
    sys.exit("falsify: no update_attributes row in the corpus to fabricate")
json.dump(d, open(p, "w"), sort_keys=True, separators=(",", ":"))
EOP
out="$(cd "$CIROOT" && python3 ci/order-selfconsistency.py --verify-extract 2>&1)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -c 'DISAGREES' >/dev/null; then
    arm "--verify-extract catches a fabricated corpus row" "red (1)  OK"
else arm "--verify-extract catches a fabricated corpus row" "rc=$rc  FAIL"; fail=1; fi
restore

# P8 ...and it refuses rather than passing when it has nothing to check
#    against, which is the shape every other arm in this file is about.
out="$(cd "$CIROOT" && HOME="$EMPTYHOME" python3 ci/order-selfconsistency.py --verify-extract 2>&1)"; rc=$?
if [ "$rc" -eq 3 ]; then arm "--verify-extract with no recordings on the box" "REFUSED (3)  OK"
else arm "--verify-extract with no recordings on the box" "rc=$rc  FAIL"; fail=1; fi

# P9 A REAL VERDICT MOVE IS STILL RED and says MOVED. The repair must not have
#    been "ignore everything".
python3 - "$PINF" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
for f in d["findings"]:
    for r in f.get("rows") or []:
        r["verdict"] = "SELF-CONSISTENT" if r["verdict"] != "SELF-CONSISTENT" else "NOT-SELF-CONSISTENT"
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
out="$(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check 2>&1)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -c 'MOVED' >/dev/null; then
    arm "a pinned verdict really moved" "red (1), says MOVED  OK"
else arm "a pinned verdict really moved" "rc=$rc  FAIL"; fail=1; fi
restore

# P10 a row appearing is red too and must NOT be called a moved verdict.
python3 - "$PINF" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p))
for f in d["findings"]:
    if f.get("rows"):
        f["rows"] = f["rows"][1:]
        break
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
out="$(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check 2>&1)"; rc=$?
if [ "$rc" -eq 1 ] && printf '%s' "$out" | grep -c 'APPEARED' >/dev/null \
   && ! printf '%s' "$out" | grep -c 'MOVED' >/dev/null; then
    arm "a row appears and is NOT reported as a moved verdict" "red (1), says APPEARED  OK"
else arm "a row appears and is NOT reported as a moved verdict" "rc=$rc  FAIL"; fail=1; fi
restore

# P11 --check may not be pointed at an arbitrary population.
(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check --recordings "$PAT" > /dev/null 2>&1); rc=$?
if [ "$rc" -eq 3 ]; then arm "--check refuses --recordings" "REFUSED (3)  OK"
else arm "--check refuses --recordings" "rc=$rc  FAIL"; fail=1; fi

# P12 a pin declaring no population is refused rather than falling back.
python3 - "$PINF" <<'EOP'
import json, sys
p = sys.argv[1]; d = json.load(open(p)); d.pop("population", None)
json.dump(d, open(p, "w"), indent=2, sort_keys=True)
EOP
(cd "$CIROOT" && python3 ci/order-selfconsistency.py --check > /dev/null 2>&1); rc=$?
if [ "$rc" -eq 3 ]; then arm "a pin declaring no population is refused" "REFUSED (3)  OK"
else arm "a pin declaring no population is refused" "rc=$rc  FAIL"; fail=1; fi
restore

if [ "$fail" -ne 0 ]; then echo "order-selfconsistency-falsify: FAILED"; exit 1; fi
echo "order-selfconsistency-falsify: 5 injected faults refused, 2 negative controls green;" \
     "corpus: 9 injected faults refused or red, 2 controls green, 1 before/after pair" \
     "(the arm that matters: no recording reachable anywhere, still green)"
