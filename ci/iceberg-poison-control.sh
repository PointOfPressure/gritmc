#!/usr/bin/env bash
# iceberg-poison-control.sh — prove the seed-12345 window actually routes
# through the `IcebergFeature` port before believing any number taken on it.
#
# WHY THIS EXISTS, AND WHY IT POISONS A *DRAW* AND NOT A BLOCK
#
# Three poison controls on 2026-08-09 came back byte-identical, because
# nothing in the window they were run on routed the rule through the tested
# path. A control that cannot fire is not a control, and the honest outcome
# of one is to say so. So this file's job is not to show that the port
# works — the census does that — it is to show that the *instrument* is
# looking at the port.
#
# The obvious poison is to stop writing the block and watch `blue_ice` fall
# to zero. That is a weak control: it proves only that a feature which writes
# blue ice writes blue ice, and it would pass just as happily if the port
# were wrong in every other respect.
#
# So the poison here is a **draw-budget** change with no intended geometry in
# it at all. `IcebergFeature#heightDependentRadiusRound` evaluates
#
#     if (l > 15 + random.nextInt(5))
#
# and the `nextInt(5)` is the right operand of a comparison, so vanilla
# consumes it on **every** call whether or not the branch is taken. The
# poison hoists that draw inside the branch. Nothing about the intended shape
# changes; only the position of the RNG stream does, and only on the round
# arm. If the census moves, the stream really is threaded through this code
# and the window really does contain round icebergs. If it does not move,
# either the round arm never runs here or the measurement is not reaching the
# feature — and both of those are findings that have to be reported rather
# than papered over.
#
# It also fires a SECOND, coarser poison as a floor on the first: the
# `signedDistanceCircle` roughness draw removed entirely. If the fine poison
# is silent and the coarse one is silent too, nothing here is live.
#
# The tree is restored from a byte copy taken before the first edit, on the
# way out and including on failure, so a killed run does not leave a poisoned
# worktree behind. A copy rather than `git checkout` because this runs in the
# rsynced remote tree, which has no `.git` — and a restore that silently does
# nothing there would leave the next agent measuring a poisoned generator.
#
# Usage:  ./ci/iceberg-poison-control.sh
# Env:    POISON_OUT   scratch root                (default ~/blueice-work/poison)
#         POISON_REFS  read-only reference root    (default ~/rustmc/oracle-work)
#         RUSTMC_REMOTE_DIR must already hold this tree, built.
#
# Vanilla is not consulted at all: this is our dump against our dump, which
# is what makes it a control rather than a measurement.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

OUT=${POISON_OUT:-$HOME/blueice-work/poison}
SRC=crates/rustmc-worldgen/src/feature/iceberg.rs
D=./target/release/worldgen-dump
G=./target/release/region-differ
BBOX="48 48 80 80"
SEED=12345

[ -f "$SRC" ] || { echo "no $SRC — wrong tree?" >&2; exit 2; }
if git rev-parse --git-dir >/dev/null 2>&1; then
    git diff --quiet -- "$SRC" || {
        echo "$SRC has uncommitted edits — REFUSING rather than reverting yours." >&2
        exit 2; }
fi

mkdir -p "$OUT"
PRISTINE="$OUT/iceberg.rs.pristine"
cp "$SRC" "$PRISTINE" || exit 2
restore() { cp "$PRISTINE" "$SRC"; }
trap restore EXIT INT TERM

# ---------------------------------------------------------------------------
# The clean dump first, so the comparison has a fixed left-hand side that was
# taken by this script rather than inherited from another run.
dump() {
    # TWO STATEMENTS, NOT ONE. `local tag=$1 dir="$OUT/$tag"` expands every
    # word before the builtin assigns any of them, so `$tag` is still unset
    # when `$dir` is built and `set -u` kills the script on line one.
    local tag=$1
    local dir="$OUT/$tag"
    rm -rf "$dir"
    # `full`, not `overworld` — the subcommand names the *extent*, and
    # `worldgen-dump` answers an unknown one with a note about noise
    # parameters that reads like a data problem rather than a usage error.
    RUSTMC_DECORATE_ORDER=zx $D full "$SEED" 16 "$dir" 64 64 \
        > "$OUT/$tag.log" 2>&1
    local rc=$?
    local n; n=$(ls "$dir"/region/*.mca 2>/dev/null | wc -l)
    [ "$rc" = 0 ] && [ "$n" -gt 0 ] || {
        echo "  dump $tag produced nothing (rc=$rc, $n region files) — REFUSING." >&2
        tail -3 "$OUT/$tag.log" >&2; exit 2; }
    printf '  dump %-8s %s region files\n' "$tag" "$n"
}

census() {
    local tag=$1
    # shellcheck disable=SC2086
    $G census "$OUT/$tag/region" --bbox $BBOX --match blue_ice --positions \
        2>/dev/null | grep -E '^-?[0-9]+ ' | awk '{print $1,$2,$3}' | sort \
        > "$OUT/$tag.blue"
    # The census prints `CENSUS chunks=... blocks=...` and then one
    # `<count>  <state>` row per state; there is no `total=` field, and a
    # grep for one silently yields an empty column rather than failing.
    # shellcheck disable=SC2086
    $G census "$OUT/$tag/region" --bbox $BBOX --match packed_ice 2>/dev/null \
        | awk '$2=="minecraft:packed_ice"{print $1}' | head -1 > "$OUT/$tag.packed"
    printf '  %-8s blue_ice %8s   packed_ice %10s\n' \
        "$tag" "$(wc -l < "$OUT/$tag.blue")" "$(cat "$OUT/$tag.packed" 2>/dev/null || echo ?)"
}

build() {
    cargo build --release -p worldgen-dump > "$OUT/build.log" 2>&1 || {
        echo "  build failed — see $OUT/build.log" >&2; exit 2; }
}

echo "=== CLEAN ==="
restore; build; dump clean; census clean

# ---------------------------------------------------------------------------
# POISON 1 — the unconditional draw made conditional. Pure stream position.
echo "=== POISON 1: heightDependentRadiusRound's nextInt(5) hoisted into the branch ==="
python3 - "$SRC" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1]); s = p.read_text()
old = "    if height > 15 + random.next_i32_bound(5) {"
new = "    if height > 15 && height > 15 + random.next_i32_bound(5) {"
assert s.count(old) == 1, "poison 1 anchor moved — the control cannot be trusted"
p.write_text(s.replace(old, new))
PY
[ $? = 0 ] || exit 2
build; dump poison1; census poison1

# ---------------------------------------------------------------------------
# POISON 2 — the roughness draw removed outright. Coarse, and only consulted
# if poison 1 is silent.
echo "=== POISON 2: signedDistanceCircle's roughness draw removed ==="
restore
python3 - "$SRC" <<'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1]); s = p.read_text()
old = "    let jitter = 10.0 * mth_clamp(random.next_f32(), 0.2, 0.8) / radius as f32;"
new = "    let jitter = 10.0 * 0.5_f32 / radius as f32;\n    let _ = &random;"
assert s.count(old) == 1, "poison 2 anchor moved — the control cannot be trusted"
p.write_text(s.replace(old, new))
PY
[ $? = 0 ] || exit 2
build; dump poison2; census poison2

restore

# ---------------------------------------------------------------------------
echo
echo "=== VERDICT ==="
clean_n=$(wc -l < "$OUT/clean.blue")
for tag in poison1 poison2; do
    n=$(wc -l < "$OUT/$tag.blue")
    moved=$(comm -3 "$OUT/clean.blue" "$OUT/$tag.blue" | wc -l)
    if [ "$moved" -eq 0 ]; then
        printf '  %-8s blue_ice %8s (clean %s)  positions moved: 0  -> CANNOT FIRE\n' \
            "$tag" "$n" "$clean_n"
    else
        printf '  %-8s blue_ice %8s (clean %s)  positions moved: %s  -> FIRES\n' \
            "$tag" "$n" "$clean_n" "$moved"
    fi
done
echo
echo "  A control that cannot fire is not a control. If both read CANNOT FIRE,"
echo "  no number taken on this window says anything about this port."
echo "POISON CONTROL DONE"
