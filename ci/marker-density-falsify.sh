#!/usr/bin/env bash
# marker-density-falsify.sh — inject faults, assert ci/marker-density.py catches each.
#
# The claim is that a refusal-marker census tracks how much code is WRITTEN.
# The two ways to hold that claim without earning it are a dead regex (no
# markers, so no correlation to be wrong about) and arithmetic that produces a
# large coefficient from any two lists. Both are asserted against here.
#
# Five assertions, all in a scratch tree:
#   1  the clean copy is green
#   2  a dead marker regex fires the floor control: exit 3, nothing on stdout
#   3  too few files fires it too — the other floor
#   4  breaking the relation makes --check refuse: markers redistributed so
#      that the smallest files carry the most
#   5  the permutation control is live — it is not returning a constant p
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$HERE")"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "$SCRATCH"' EXIT
FAIL=0
note() { printf '  %-5s %s\n' "$1" "$2"; }
ok()   { note "ok" "$1"; }
bad()  { note "FAIL" "$1"; FAIL=1; }

mkdir -p "$SCRATCH/ci" "$SCRATCH/crates/rustmc-server/src"
cp "$HERE/marker-density.py" "$SCRATCH/ci/"
cp "$ROOT/crates/rustmc-server/src/"*.rs "$SCRATCH/crates/rustmc-server/src/"
MD="$SCRATCH/ci/marker-density.py"
cp "$MD" "$SCRATCH/md.orig"

echo "== 1: the clean copy is green"
if python3 "$MD" --check --permutations 2000 >/dev/null 2>&1; then
    ok "--check green"
else
    bad "--check red on a clean copy — every later assertion is vacuous"
fi

echo "== 2: a dead marker regex fires the floor control"
python3 - "$MD" <<'PY'
import re,sys
p=sys.argv[1]; s=open(p).read()
s=re.sub(r"MARKER = re\.compile\(\n(?:.*\n)*?.*re\.I\)",
         "MARKER = re.compile(r'zzz_falsify_no_such_marker')", s, count=1)
open(p,"w").write(s)
PY
OUT="$(python3 "$MD" 2>/dev/null)"; RC=$?
ERR="$(python3 "$MD" 2>&1 >/dev/null)"
if [ $RC -eq 3 ] && [ -z "$OUT" ]; then ok "exit 3, nothing printed"
else bad "a dead regex still produced a coefficient (rc=$RC)"; fi
if printf '%s' "$ERR" | grep -c "markers" >/dev/null; then ok "named the empty population"
else bad "refused without saying what was empty"; fi
cp "$SCRATCH/md.orig" "$MD"

echo "== 3: too few files fires it too"
python3 - "$MD" <<'PY'
import sys
p=sys.argv[1]; s=open(p).read()
s=s.replace('SCAN = "crates/rustmc-server/src/*.rs"',
            'SCAN = "crates/rustmc-server/src/e*.rs"')
open(p,"w").write(s)
PY
python3 "$MD" >/dev/null 2>&1; RC=$?
if [ $RC -eq 3 ]; then ok "exit 3 on too few files"; else bad "expected exit 3, got $RC"; fi
cp "$SCRATCH/md.orig" "$MD"

echo "== 4: breaking the relation makes --check refuse"
# Give the SHORTEST files a marker each and strip them from the longest. If the
# check still passes, it is not reading the relation it claims to.
python3 - "$SCRATCH/crates/rustmc-server/src" <<'PY'
import glob,os,re,sys
d=sys.argv[1]
MARK=re.compile(r'unported|not ported|todo!|unimplemented|TODO|FIXME|not implemented|deliberately not|is a no-op|are a no-op',re.I)
fs=sorted(glob.glob(os.path.join(d,'*.rs')), key=lambda p: len(open(p).read().splitlines()))
for p in fs[:20]:
    with open(p,'a') as f:
        for i in range(12): f.write('\n// TODO falsify marker %d\n'%i)
for p in fs[-12:]:
    src=open(p).read().splitlines()
    open(p,'w').write("\n".join(ln for ln in src if not MARK.search(ln)))
PY
if python3 "$MD" --check --permutations 2000 >/dev/null 2>&1; then
    bad "the relation was inverted and --check stayed green"
else
    ok "refused"
fi
rm -f "$SCRATCH/crates/rustmc-server/src/"*.rs
cp "$ROOT/crates/rustmc-server/src/"*.rs "$SCRATCH/crates/rustmc-server/src/"

echo "== 5: the permutation control is live"
P1="$(python3 "$MD" --permutations 200 2>/dev/null | sed -n 's/.*p = \([0-9.]*\) over.*/\1/p')"
P2="$(python3 "$MD" --permutations 200 --seed 1 2>/dev/null | sed -n 's/.*p = \([0-9.]*\) over.*/\1/p')"
P3="$(python3 "$MD" --permutations 20000 2>/dev/null | sed -n 's/.*p = \([0-9.]*\) over.*/\1/p')"
if [ -n "$P1" ] && [ -n "$P3" ] && [ "$P1" != "$P3" ]; then
    ok "p moves with the permutation count ($P1 at 200, $P3 at 20000)"
else
    bad "p is constant across permutation counts ($P1 vs $P3) — it is not being computed"
fi
if [ -n "$P2" ]; then ok "a second seed also produces a p ($P2)"; else bad "seeded run produced no p"; fi

echo
if [ $FAIL -eq 0 ]; then
    echo "marker-density-falsify: all assertions held"
else
    echo "marker-density-falsify: RED — an injected fault went unrefused" >&2
fi
exit $FAIL
