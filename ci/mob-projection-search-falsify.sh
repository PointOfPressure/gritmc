#!/usr/bin/env bash
# mob-projection-search-falsify.sh — inject faults, assert the search catches each.
#
# The search reports a table of fractions and two of its rows are 0/120 and
# 120/120. Both extremes are what a broken search produces for free: a
# comparator that always returns "different" gives 0, one that always returns
# "same" gives the maximum, and a population of one gives 0/0 printed as 0.0%.
# So all three are asserted against.
#
# Six assertions, in a scratch tree:
#   1  the clean copy is green and the two extreme rows are really extremes
#   2  a population below the floor refuses: exit 3, nothing on stdout
#   3  the comparator discriminates — a projection known to differ is not
#      reported identical, and one known to be identical is not reported
#      different (both directions, from the same run set)
#   4  dropping the yaw fields is what moves {add_entity}, not the drop
#      machinery: dropping an unrelated field leaves it at 0
#   4b --check reads the COMMITTED corpus: no recordings on the box leaves it
#      green, a missing corpus refuses, a too-small population still refuses
#   5  --check refuses when a result's SHAPE changes
#   6  --check does NOT refuse when only the fraction moves, which is the
#      whole reason it gates shape
#   7  pairs are taken within one recorder vocabulary: a recording with one
#      more add_entity field is not vanilla disagreeing with itself
#   8  a foreign tree's recording moves the box search and not --check; a
#      --check that searches the box is caught by the same arm
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
FAIL=0
note() { printf '  %-5s %s\n' "$1" "$2"; }
ok()   { note "ok" "$1"; }
bad()  { note "FAIL" "$1"; FAIL=1; }

mkdir -p "$SCRATCH/ci/checks"
cp "$HERE/mob-projection-search.py" "$SCRATCH/ci/"
cp "$HERE/checks/mob-projection.json" "$SCRATCH/ci/checks/"
cp "$HERE/checks/mob-projection-corpus.json" "$SCRATCH/ci/checks/"
PS="$SCRATCH/ci/mob-projection-search.py"
LED="$SCRATCH/ci/checks/mob-projection.json"
cp "$PS" "$SCRATCH/ps.orig"
cp "$LED" "$SCRATCH/led.orig"

get() { python3 "$PS" --json 2>/dev/null | python3 -c "import json,sys;print(json.load(sys.stdin)['results']['$1'])" 2>/dev/null; }

echo "== 1: the clean copy, and the two extremes are extremes"
if python3 "$PS" --check >/dev/null 2>&1; then ok "--check green"
else bad "--check red before any fault"; fi
A="$(get 'observe as declared')"
B="$(get '{add_entity} minus y_rot + y_head_rot')"
if printf '%s' "$A" | grep -cE '^\[0, [0-9]+\]$' >/dev/null; then ok "declared projection is 0 of N"
else bad "expected 0 of N for the declared projection, got '$A'"; fi
if printf '%s' "$B" | python3 -c 'import sys,json; v=json.loads(sys.stdin.read()); sys.exit(0 if v[0]==v[1] and v[1]>0 else 1)'; then
    ok "yaw-dropped add_entity is N of N"
else bad "expected N of N, got '$B'"; fi

echo "== 2: too small a population refuses"
python3 - "$PS" <<'PY'
import sys
p=sys.argv[1]; s=open(p).read()
s=s.replace('if fig["current_schema_recordings"] < 5 or fig["pairs"] < 10:',
            'if fig["current_schema_recordings"] < 9999 or fig["pairs"] < 10:')
open(p,"w").write(s)
PY
OUT="$(python3 "$PS" 2>/dev/null)"; RC=$?
if [ $RC -eq 3 ] && [ -z "$OUT" ]; then ok "exit 3, nothing printed"
else bad "a too-small population still printed a table (rc=$RC)"; fi
cp "$SCRATCH/ps.orig" "$PS"

echo "== 3: the comparator discriminates in both directions"
# Both rows come out of the SAME run set through the SAME comparator. If it
# were stuck on "different" row B could not be N/N; if it were stuck on "same"
# row A could not be 0/N. Having both extremes present from one population is
# the two-sided control, and it is why assertion 1 checks both rather than one.
if printf '%s' "$A" | grep -cE '^\[0,' >/dev/null && printf '%s' "$B" | python3 -c 'import sys,json;v=json.loads(sys.stdin.read());sys.exit(0 if v[0]==v[1] else 1)'; then
    ok "same population, same comparator, 0/N and N/N — not stuck either way"
else
    bad "the comparator did not produce both extremes; one of the rows proves nothing"
fi

echo "== 4: it is the yaw fields that move it, not the drop machinery"
python3 - "$PS" <<'PY'
import sys
p=sys.argv[1]; s=open(p).read()
s=s.replace('''    ("{add_entity} minus y_rot + y_head_rot", ("add_entity",),
     ("y_rot", "y_head_rot")),''','''    ("{add_entity} minus y_rot + y_head_rot", ("add_entity",),
     ("kind_id", "at")),''')
open(p,"w").write(s)
PY
C="$(get '{add_entity} minus y_rot + y_head_rot')"
# The bar is "not N of N", not "exactly 0 of N". Dropping `kind_id` and `at`
# leaves the baseline agreement of `{add_entity}` alone, which is 1/120 and not
# 0/120 — two recordings happen to have drawn the same yaw. The first version
# of this assertion demanded 0 and failed on that 1, correctly: the claim is
# that the yaw is what takes the projection to exact, and the falsifiable form
# of that is that dropping anything else does not.
if printf '%s' "$C" | python3 -c 'import sys,json; v=json.loads(sys.stdin.read()); sys.exit(0 if v[0]!=v[1] else 1)'; then
    ok "dropping kind_id and at leaves it at $C, not N/N — the yaw is what mattered"
else
    bad "dropping kind_id and at also made it exact ('$C'); the drop is doing the work"
fi
cp "$SCRATCH/ps.orig" "$PS"

echo "== 4b: --check reads the committed corpus: an empty box does not move it, a missing corpus refuses"
python3 - "$PS" <<'PY'
import sys
p=sys.argv[1]; s=open(p).read()
s=s.replace('"/home/sayer/rustmc*/oracle-work/gameplay*/vanilla.json"',
            '"/nonexistent-falsify/*/vanilla.json"')
open(p,"w").write(s)
PY
OUT="$(python3 "$PS" --check 2>&1)"; RC=$?
if [ $RC -eq 0 ] && printf '%s' "$OUT" | grep -c "shapes unchanged" >/dev/null; then
    ok "no recordings on the box: --check still measures the corpus and is green"
else
    bad "an empty box moved --check (rc=$RC): $OUT"
fi
mv "$SCRATCH/ci/checks/mob-projection-corpus.json" "$SCRATCH/corpus.hold"
python3 "$PS" --check >/dev/null 2>&1; RC=$?
if [ $RC -eq 2 ]; then ok "no committed corpus: REFUSED (2), no fallback to the box"
else bad "a missing corpus gave rc=$RC, not a refusal"; fi
mv "$SCRATCH/corpus.hold" "$SCRATCH/ci/checks/mob-projection-corpus.json"
cp "$SCRATCH/ps.orig" "$PS"
python3 - "$PS" <<'PY'
import sys
p=sys.argv[1]; s=open(p).read()
s=s.replace('if fig["current_schema_recordings"] < 5 or fig["pairs"] < 10:',
            'if fig["current_schema_recordings"] < 9999 or fig["pairs"] < 10:')
open(p,"w").write(s)
PY
python3 "$PS" --check >/dev/null 2>&1; RC=$?
if [ $RC -eq 3 ]; then ok "a non-empty but too-small population still refuses"
else bad "expected exit 3 for a small population, got $RC"; fi
cp "$SCRATCH/ps.orig" "$PS"

echo "== 5: --check refuses a changed shape"
python3 - "$LED" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p))
d["results"]["observe as declared"]=[5,120]     # never -> sometimes
json.dump(d,open(p,"w"),indent=1,sort_keys=True)
PY
if python3 "$PS" --check >/dev/null 2>&1; then
    bad "a never -> sometimes change went unrefused"
else
    ok "refused"
fi
cp "$SCRATCH/led.orig" "$LED"

echo "== 6: --check tolerates a moved fraction of the same shape"
python3 - "$LED" <<'PY'
import json,sys
p=sys.argv[1]; d=json.load(open(p))
k="observe, minus y_rot + y_head_rot"
d["results"][k]=[d["results"][k][0]+3, d["results"][k][1]]   # sometimes -> sometimes
json.dump(d,open(p,"w"),indent=1,sort_keys=True)
PY
if python3 "$PS" --check >/dev/null 2>&1; then
    ok "not refused — it gates the shape, so making a tree does not redden T0"
else
    bad "refused on a fraction move; this gate will be turned off within a week"
fi
cp "$SCRATCH/led.orig" "$LED"

echo "== 7: pairs are taken within one recorder vocabulary"
# 2026-09-24: the recorder on main 26ffd4965 decodes add_entity's `movement`
# as well, and the first recording it made on this box disagreed with every
# other on the field NAMES. The shipped rule split recordings only on whether
# add_entity carried `data`, so `{add_entity} minus y_rot + y_head_rot` went
# 120/120 -> 528/561 and reddened every T0 on the box. Reconstructed here from
# six real recordings plus a seventh that is one of them with `movement` added.
mkdir -p "$SCRATCH/rec"
python3 - "$PS" "$SCRATCH/rec" <<'PY'
import importlib.util, json, os, sys, copy
spec = importlib.util.spec_from_file_location("m", sys.argv[1]); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
rows, _ = m.load()
groups = {}
for r in rows:
    if r[1] != "older":
        groups.setdefault(r[1], []).append(r[2])
big = max(groups.values(), key=len)[:6]
out = sys.argv[2]
for i, obs in enumerate(big):
    os.makedirs(os.path.join(out, "r%d" % i), exist_ok=True)
    json.dump({"scenarios": [{"name": m.SCENARIO, "obs": obs}]}, open(os.path.join(out, "r%d" % i, "vanilla.json"), "w"))
mut = copy.deepcopy(big[0])
for o in mut:
    if o["kind"] == "add_entity":
        o["fields"]["movement"] = [0.0, 0.0, 0.0]
os.makedirs(os.path.join(out, "r9"), exist_ok=True)
json.dump({"scenarios": [{"name": m.SCENARIO, "obs": mut}]}, open(os.path.join(out, "r9", "vanilla.json"), "w"))
PY
python3 - "$PS" "$SCRATCH/rec" <<'PY'
import sys
p=sys.argv[1]; s=open(p).read()
s=s.replace('"/home/sayer/rustmc*/oracle-work/gameplay*/vanilla.json"', repr(sys.argv[2] + "/*/vanilla.json"))
open(p,"w").write(s)
PY
D="$(get '{add_entity} minus y_rot + y_head_rot')"
V="$(python3 "$PS" --json 2>/dev/null | python3 -c 'import json,sys;print(json.load(sys.stdin)["recorder_vocabularies"])' 2>/dev/null)"
if [ "$V" = 2 ] && printf '%s' "$D" | python3 -c 'import sys,json; v=json.loads(sys.stdin.read()); sys.exit(0 if v[0]==v[1] and v[1]>0 else 1)'; then
    ok "a seventh recording with one more add_entity field is its own vocabulary, and the row stays N of N ($D)"
else
    bad "vocabularies=$V, row=$D: a decoder difference is being read as vanilla disagreeing with itself"
fi
python3 - "$PS" <<'PY'
import re, sys
p=sys.argv[1]; s=open(p).read()
s2=re.sub(r'schema = \(tuple\(sorted\(ae\[0\]\["fields"\]\)\) if ae and "data" in ae\[0\]\["fields"\]\s*else "older"\)',
          'schema = "current" if ae and "data" in ae[0]["fields"] else "older"', s)
assert s2 != s, "the vocabulary rule was not found to revert"
open(p,"w").write(s2)
PY
E="$(get '{add_entity} minus y_rot + y_head_rot')"
if printf '%s' "$E" | python3 -c 'import sys,json; v=json.loads(sys.stdin.read()); sys.exit(0 if v[0]!=v[1] else 1)'; then
    ok "the shipped rule, restored, reads the same population as disagreement ($E)"
else
    bad "the shipped rule still reads N of N ('$E'), so case 7 does not reconstruct the defect"
fi
cp "$SCRATCH/ps.orig" "$PS"

echo "== 8: a foreign tree's recording cannot move --check"
# The 23-24 Sep defect: one tree's recording reddened every T0 on the box. A
# scratch box holding the corpus's own recordings plus ONE crafted foreign one
# in which update_attributes differs: the ad-hoc box search must see
# '{update_attributes} alone' go from always to sometimes (the recording is
# potent), and --check must print exactly what it prints without it. Run
# against a copy whose --check reads the box, the same arm must say "moved".
foreign() {  # checker source -> same | moved | impotent
    local F="$SCRATCH/foreign" a b c
    rm -rf "$F"; mkdir -p "$F/ci/checks" "$F/box"
    cp "$SCRATCH/ci/checks/"*.json "$F/ci/checks/"
    python3 - "$1" "$F/ci/mob-projection-search.py" "$F/box" "$F/ci/checks/mob-projection-corpus.json" <<'PY'
import sys, json, os, copy
s=open(sys.argv[1]).read()
s=s.replace('"/home/sayer/rustmc*/oracle-work/gameplay*/vanilla.json"', repr(sys.argv[3] + "/*/vanilla.json"))
open(sys.argv[2],"w").write(s)
d=json.load(open(sys.argv[4]))
recs=[r for r in d["recordings"] if r["schema"] != "older"]
big=max({tuple(r["schema"]) for r in recs}, key=lambda k: sum(1 for r in recs if tuple(r["schema"])==k))
recs=[r for r in recs if tuple(r["schema"])==big]
for i, r in enumerate(recs):
    os.makedirs(os.path.join(sys.argv[3], "t%d" % i))
    json.dump({"scenarios":[{"name":"spawn-egg-makes-a-mob","obs":r["obs"]}]}, open(os.path.join(sys.argv[3], "t%d" % i, "vanilla.json"),"w"))
PY
    a="$(python3 "$F/ci/mob-projection-search.py" --check 2>&1; echo "rc=$?")"
    python3 - "$F/box" <<'PY'
import sys, json, os, copy
d=json.load(open(os.path.join(sys.argv[1], "t0", "vanilla.json")))
obs=copy.deepcopy(d["scenarios"][0]["obs"])
hit=False
for o in obs:
    if o["kind"]=="update_attributes":
        o["fields"]["foreign"]=1; hit=True
if not hit:
    obs.append({"who":"actor","kind":"update_attributes","at_ms":0,"fields":{"foreign":1}})
os.makedirs(os.path.join(sys.argv[1], "zforeign"))
json.dump({"scenarios":[{"name":"spawn-egg-makes-a-mob","obs":obs}]}, open(os.path.join(sys.argv[1], "zforeign", "vanilla.json"),"w"))
PY
    b="$(python3 "$F/ci/mob-projection-search.py" --check 2>&1; echo "rc=$?")"
    c="$(python3 "$F/ci/mob-projection-search.py" --json 2>/dev/null | python3 -c 'import json,sys;v=json.load(sys.stdin)["results"]["{update_attributes} alone"];print("always" if v[0]==v[1] else "not-always")')"
    if [ "$c" != not-always ]; then echo impotent
    elif [ "$a" = "$b" ]; then echo same; else echo moved; fi
}
R="$(foreign "$PS")"
if [ "$R" = same ]; then ok "the box search moves on a foreign recording; --check prints the same bytes"
else bad "foreign recording arm: $R"; fi
python3 - "$PS" "$SCRATCH/globcheck.py" <<'PY'
import sys
s=open(sys.argv[1]).read()
a="        fig = figures(corpus)\n"
assert a in s, "arm 8 mutant lost its anchor"
open(sys.argv[2],"w").write(s.replace(a, "        fig = figures()\n"))
PY
R="$(foreign "$SCRATCH/globcheck.py")"
if [ "$R" = moved ]; then ok "MUTANT: a --check that searches the box is caught here"
else bad "MUTANT: a box-reading --check was not caught ($R)"; fi

echo
if [ $FAIL -eq 0 ]; then
    echo "mob-projection-search-falsify: all assertions held"
else
    echo "mob-projection-search-falsify: RED — an injected fault went unrefused" >&2
fi
exit $FAIL
