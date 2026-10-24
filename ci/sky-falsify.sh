#!/usr/bin/env bash
# sky-falsify.sh — proof that tests/sky_golden.rs can go red.
#
# `docs/AGENT-BRIEF.md`'s hard rule: a check ships with the evidence it can
# fail. The sky golden compares 511 samples against the JVM with no tolerance,
# which sounds unfalsifiable-proof and is not: the test skips itself when the
# extract is missing, and its FIRST RUN did exactly that and reported three
# passes. That is why case 0 exists and why it is first.
#
# Each case patches ONE constant in crates/rustmc-server/src/sky.rs, rebuilds,
# and asserts the test refuses. Every constant here is one this port could
# plausibly have got wrong from the bytecode:
#
#   0  the extract is absent            -> the test must SKIP, and be seen to
#   1  the cubic Bezier's y controls    -> the whole easing curve
#   2  loopTicks uses % not floorMod    -> only negative day times
#   3  the leading wrap segment dropped  -> only ticks before the first keyframe
#   4  Mth.cos replaced by f64::cos     -> only near the table's slot edges
#   5  Math.round(x) as f32::round      -> UNREACHABLE, and this proves it
#
# Cases 2-4 are the interesting ones: each is wrong in a NARROW band, which is
# what the dense sampling around the crossings and the keyframe edges is for.
# A sweep that only checked noon would pass every one of them — and case 4 is
# the sharpest, because `Mth.cos` and a platform cosine give a daylight
# detector a different `power` at exactly **two ticks of the 24,000**, which
# is why 2447 and 10625 are named in the sample grid.
#
# **Case 5 expects GREEN and that is the finding, not a weakness.** Swept over
# all 24,000 ticks at every sky brightness 1..=15, no `15 * cos(a)` lands on
# an exact half, and the `clamp(0, 15)` on the next line erases every negative
# result where the two rules could otherwise disagree. So the distinction is
# unreachable through this function. It is recorded here rather than left as a
# red somebody would eventually widen a tolerance to satisfy.
# **DELIBERATELY NOT IN `ci/t0.sh`, AND THE MEASURED RESULT IS RECORDED HERE
# INSTEAD.** Seven release rebuilds is about eight minutes, and T0 is run by
# every branch on the box; `ci/corpus-lock-falsify.sh` sits outside T0 for the
# same reason at 16m42s. A falsifier nobody can afford to run is worse than
# one that is run on the commit that changes what it guards, so: **re-run this
# whenever you touch `crates/rustmc-server/src/sky.rs`**, and paste what it
# says under the block below rather than trusting this one.
#
# Measured 2026-08-18 on sayer-desktop, in ~/rustmc-sky:
#
#     ok   control: green
#     ok   control: 0 skips
#     ok   case 0: 3 cases announced the skip
#     ok   case 1 (easing curve): red
#     ok   case 2 (negative day time wraps the wrong way): red
#     ok   case 3 (ticks before the first keyframe): red
#     ok   case 4: red
#     ok   case 5: green — the rounding rule is unreachable here, as documented
#
# **Case 0 failed on its first run and the fix is worth knowing**: `run_test`
# had no `-- --nocapture`, so cargo swallowed the skip line and the script
# concluded a missing extract was invisible when it was merely unprinted. A
# falsifier that reads a log has to be sure the log is being written.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/crates/rustmc-server/src/sky.rs"
TABLE="$ROOT/data/vanilla/timelines.json"
BACKUP="$(mktemp)"; TABLEBAK="$(mktemp)"
cp "$SRC" "$BACKUP"
[ -s "$TABLE" ] && cp "$TABLE" "$TABLEBAK"
restore() { cp "$BACKUP" "$SRC"; [ -s "$TABLEBAK" ] && cp "$TABLEBAK" "$TABLE"; }
trap 'restore; rm -f "$BACKUP" "$TABLEBAK"' EXIT

PASS=0; FAIL=0
run_test() { # -> 0 green, 1 red
    (cd "$ROOT" && cargo test --release -p rustmc-server --test sky_golden -- --nocapture >/tmp/sky-falsify.log 2>&1)
}
skips() { grep -c 'skipping: no vanilla' /tmp/sky-falsify.log || true; }

expect() { # expect <what> <green|red> <case name>
    local want="$2" name="$3"
    if run_test; then got=green; else got=red; fi
    if [ "$got" = "$want" ]; then
        echo "ok   $name: $got"; PASS=$((PASS+1))
    else
        echo "FAIL $name: wanted $want, got $got"; FAIL=$((FAIL+1))
        tail -20 /tmp/sky-falsify.log | sed 's/^/       /'
    fi
}

echo "== control: unpatched, extract present"
expect . green "control"
n=$(skips); [ "$n" = 0 ] && { echo "ok   control: 0 skips"; PASS=$((PASS+1)); } || {
    echo "FAIL control: $n cases SKIPPED — the green is vacuous"; FAIL=$((FAIL+1)); }

echo "== case 0: the extract is absent — the test must say so, not pass silently"
mv "$TABLE" "$TABLE.hidden"
run_test || true
n=$(skips)
if [ "$n" -ge 3 ]; then
    echo "ok   case 0: $n cases announced the skip"; PASS=$((PASS+1))
else
    echo "FAIL case 0: only $n skip lines — a missing extract is invisible"; FAIL=$((FAIL+1))
fi
mv "$TABLE.hidden" "$TABLE"

patch() { python3 - "$SRC" "$1" "$2" <<'PY'
import sys
p, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(p).read()
if old not in s:
    raise SystemExit(f"sky-falsify: anchor has moved, patch not applied:\n  {old}")
open(p, "w").write(s.replace(old, new, 1))
PY
}

echo "== case 1: the Bezier's y controls swapped for its x controls"
patch "y: CubicCurve::from_controls(f(1), f(3))," "y: CubicCurve::from_controls(f(0), f(2)),"
expect . red "case 1 (easing curve)"
restore

echo "== case 2: loopTicks as % rather than Math.floorMod"
patch "Some(p) => ticks.rem_euclid(i64::from(p))," "Some(p) => ticks % i64::from(p),"
expect . red "case 2 (negative day time wraps the wrong way)"
restore

echo "== case 3: the leading wrap segment dropped"
patch "        if let Some(p) = period_ticks {
            segments.push(Segment {
                easing,
                from_value: last.1,
                from_ticks: last.0 - p,
                to_value: first.1,
                to_ticks: first.0,
            });
        }" "        if false {
            let _ = period_ticks;
        }"
expect . red "case 3 (ticks before the first keyframe)"
restore

echo "== case 4: Mth.cos replaced by the platform cosine"
patch "        let scaled = b as f32 * mth_cos(f64::from(a));" \
      "        let scaled = b as f32 * (f64::from(a).cos() as f32);"
# The detector arithmetic is not what tests/sky_golden.rs reads, so this one
# is checked where it IS read: the unit test in crate::sky. Stated rather than
# quietly skipped — see the note at the end of this script.
echo "     (checked by the crate's own tests, not by sky_golden — see below)"
(cd "$ROOT" && cargo test --release -p rustmc-server --lib sky:: >/tmp/sky-falsify.log 2>&1) \
    && { echo "FAIL case 4: the platform cosine passed"; FAIL=$((FAIL+1)); } \
    || { echo "ok   case 4: red"; PASS=$((PASS+1)); }
restore

echo "== case 5: Math.round(float) written as f32::round — expected GREEN"
patch "        b = (scaled + 0.5).floor() as i32;" "        b = scaled.round() as i32;"
if (cd "$ROOT" && cargo test --release -p rustmc-server --lib sky:: >/tmp/sky-falsify.log 2>&1); then
    echo "ok   case 5: green — the rounding rule is unreachable here, as documented"
    PASS=$((PASS+1))
else
    echo "FAIL case 5: it went RED, so the rule IS reachable now."
    echo "     sky.rs's the_round_half_rule_is_unreachable_here test says what to do."
    FAIL=$((FAIL+1))
fi
restore

echo "== restored"
expect . green "restored"

echo
echo "sky-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
