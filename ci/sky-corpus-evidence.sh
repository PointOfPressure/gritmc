#!/usr/bin/env bash
# sky-corpus-evidence.sh — 711 shown RED against the unported code, then GREEN.
#
# `docs/AGENT-BRIEF.md`: "a check written after its fix has never been observed
# failing." This is that observation for
# `711-a-daylight-detector-recomputes-its-power-from-the-sky`, and it is a
# script rather than a paragraph so the next person can re-take it.
#
# WHAT "UNPORTED" MEANS HERE, PRECISELY
#
# `crate::sky::signal_strength_write` is made to answer `None` — the one
# function that decides a detector's `power`. Everything else stays: the
# extract loads, the sun angle is computed, the ticker list is maintained. So
# the red is attributable to *running `updateSignalStrength`* and not to the
# sky model failing to load, which a cruder patch would have conflated.
#
# The pair is run on `ci/t2-gameplay-subset.sh` — one minute of corpus lock per
# side rather than forty — and `710` is carried alongside `711` in both runs.
# `710` must MATCH in both, which is the control: it is the check that reaches
# this gap and cannot see it, so a `710` that moved would mean the patch
# changed something other than what it claims.
#
# The subset takes the box-wide corpus lock and waits its turn. Nothing to set.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/crates/rustmc-server/src/sky.rs"
BACKUP="$(mktemp)"
cp "$SRC" "$BACKUP"
trap 'cp "$BACKUP" "$SRC"; rm -f "$BACKUP"' EXIT

CHECKS=(710-a-daylight-detector-inverts-when-right-clicked.toml
        711-a-daylight-detector-recomputes-its-power-from-the-sky.toml)

echo "===== RED: crate::sky::signal_strength_write neutered ====="
python3 - "$SRC" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
# The anchor is the FIRST LINE OF THE BODY, not the signature — a signature is
# what `cargo fmt` rewrites when an argument list crosses 100 columns, and this
# script refused on exactly that once already. It refused rather than patching
# nothing, which is the behaviour to keep.
old = """pub fn signal_strength_write(state: &str, day_time: i64, sky_light: Option<i32>) -> Option<String> {
    if !is_daylight_detector(state) {"""
new = """pub fn signal_strength_write(state: &str, day_time: i64, sky_light: Option<i32>) -> Option<String> {
    // ci/sky-corpus-evidence.sh: the unported answer.
    let _ = (day_time, sky_light);
    if true {
        return None;
    }
    if !is_daylight_detector(state) {"""
if old not in s:
    raise SystemExit("sky-corpus-evidence: anchor has moved; re-point the patch")
open(p, "w").write(s.replace(old, new, 1))
PY
"$ROOT/ci/t2-gameplay-subset.sh" "${CHECKS[@]}" > "$ROOT/../sky-evidence-red.log" 2>&1 || true
cp "$BACKUP" "$SRC"

echo "===== GREEN: restored ====="
"$ROOT/ci/t2-gameplay-subset.sh" "${CHECKS[@]}" > "$ROOT/../sky-evidence-green.log" 2>&1 || true

echo
echo "===== verdicts ====="
for side in red green; do
    log="$ROOT/../sky-evidence-$side.log"
    echo "-- $side"
    grep -E "recomputes-its-power|inverts-when-right-clicked" "$log" | tail -6 | sed 's/^/     /'
done
