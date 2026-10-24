#!/usr/bin/env bash
# explosion-falsify.sh — what the explosion goldens can and cannot see.
#
#     ./ci/explosion-falsify.sh                 the whole sweep, ~10 min
#     EXPLOSION_FALSIFY_DRYRUN=1 ./ci/explosion-falsify.sh   patches only, ~1 s
#
# # Why this exists
#
# `crates/rustmc-server/tests/golden_explosion_{ray,wire,entity}.rs` compare
# the port against **vanilla's own output** — the private
# `calculateExplodedPositions` and `createFire` invoked by reflection, the real
# `STREAM_CODEC.encode`, the real `getSeenPercent`. That is the strongest form
# of golden this project has, and it is still a check whose green has to be
# shown turnable red before it means anything.
#
# It is also a check with a **measured blind spot**, and the blind spot is the
# reason this script declares outcomes per case instead of asserting that
# everything is caught. Four of the mutations below are invisible and always
# will be: they move a ray's endpoint by about 1e-7 blocks over its ~23 steps,
# and every one of these goldens records positions that have been through
# `Mth.floor`. No block-granular instrument can separate them. Declaring them
# MISSED here is the difference between a documented bound and a check that
# quietly stopped covering four constants.
#
# # How to read a failure
#
#   CAUGHT where MISSED was declared     good news, and this file is stale.
#                                        Re-measure and move the row.
#   MISSED where CAUGHT was declared     a golden has stopped seeing something
#                                        it used to. This is the failure this
#                                        script exists for.
#   INCONCLUSIVE                         the mutated tree did not build, so
#                                        nothing was measured. Exit 3.
#   PATCH MISS                           the mutation no longer applies, so it
#                                        ran nothing. A patch that has stopped
#                                        matching is a hole in the sweep
#                                        whether or not the sweep is green;
#                                        `EXPLOSION_FALSIFY_DRYRUN=1` catches
#                                        that in a second and is what to run
#                                        after touching `explosion.rs`.
#
# # What it does not do
#
# It does not restore a tree it did not manage to back up, and it does not run
# on a dirty `explosion.rs` — a mutation applied on top of uncommitted work
# would report on neither. Both are refusals, not warnings.
#
# It also does not read a failed BUILD as a caught mutation. `cargo test` exits
# non-zero for a failing assertion and equally for a build that never started,
# and the first full run of this script reported all four declared-MISSED cases
# as CAUGHT while a gate was compiling on the same box; a clean re-run of the
# same four patches reported MISSED. So the build is a separate step and its
# failure is `INCONCLUSIVE`, exit 3 — the sweep stops rather than publishing a
# green it did not earn. **Run this when the box is quiet.**
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${EXPLOSION_FALSIFY_SRC:-$ROOT/crates/rustmc-server/src/explosion.rs}"
DRYRUN="${EXPLOSION_FALSIFY_DRYRUN:-0}"

[ -f "$SRC" ] || { echo "no source at $SRC" >&2; exit 2; }

# Under DRYRUN, work on a COPY unless a source was named. Nothing is compiled,
# so the real file has no reason to be touched — and a T0 that mutated a
# tracked source, however briefly, would be one `kill -9` away from leaving a
# poisoned tree behind. The full sweep must use the real file, because the
# thing being measured is what `cargo test` does with it.
if [ "$DRYRUN" = "1" ] && [ -z "${EXPLOSION_FALSIFY_SRC:-}" ]; then
    COPY="$(mktemp)"
    cp "$SRC" "$COPY"
    SRC="$COPY"
fi

if [ "$DRYRUN" != "1" ] && [ -z "${EXPLOSION_FALSIFY_SRC:-}" ]; then
    if ! git -C "$ROOT" diff --quiet -- "$SRC"; then
        echo "REFUSED — $SRC has uncommitted changes. A mutation applied on top" >&2
        echo "of them would report on neither. Commit or stash first." >&2
        exit 2
    fi
fi

BACKUP="$(mktemp)"
cp "$SRC" "$BACKUP" || { echo "could not back up $SRC" >&2; exit 2; }
restore() { cp "$BACKUP" "$SRC"; }
trap 'restore; rm -f "$BACKUP"' EXIT

# Each case: NAME | DECLARED | FROM | TO
#
# `DECLARED` is what the goldens did on 2026-09-06, on the tree that landed
# them. The four MISSED rows are the precision ones and their reason is above.
CASES=(
"loop-order|CAUGHT|for j in 0..16i32 {
        for k in 0..16i32 {
            for l in 0..16i32 {|for l in 0..16i32 {
        for k in 0..16i32 {
            for j in 0..16i32 {"
"hashset-order|CAUGHT|    Ok(set.to_vec())|    let mut v = set.to_vec();
    v.sort_unstable();
    Ok(v)"
"resistance-plus-0.3|CAUGHT|f -= (r + 0.3) * 0.3;|f -= r * 0.3;"
"second-f-test|CAUGHT|if f > 0.0 && p.should_explode {|if p.should_explode {"
"strength-constants|CAUGHT|let mut f = radius * (0.7 + random.next_f32() * 0.6);|let mut f = radius * (0.75 + random.next_f32() * 0.5);"
"shuffle-direction|CAUGHT|        let j = random.next_i32_bound(i);|        let j = random.next_i32_bound(i) % i.max(1);
        let j = if j == i - 1 { 0 } else { j };"
"fire-draw-after-test|CAUGHT|        if random.next_i32_bound(3) != 0 {
            continue;
        }
        if !world.is_air(pos) {
            continue;
        }|        if !world.is_air(pos) {
            continue;
        }
        if random.next_i32_bound(3) != 0 {
            continue;
        }"
"seen-grid-oy|CAUGHT|    let ox = (1.0 - (1.0 / dx).floor() * dx) / 2.0;|    let ox = (1.0 - (1.0 / dx).floor() * dx) / 2.0;
    let oy_unused = (1.0 - (1.0 / dy).floor() * dy) / 2.0;
    let _ = oy_unused;"
"seen-grid-computed-count|CAUGHT|    let mut fx = 0.0f64;
    while fx <= 1.0 {|    let mut fx = 0.0f64;
    while fx < 1.0 + dx / 2.0 {"
"damage-single-radius|CAUGHT|        ((d1 * d1 + d1) / 2.0 * 7.0 * f64::from(f) + 1.0) as f32|        ((d1 * d1 + d1) / 2.0 * 7.0 * f64::from(radius) + 1.0) as f32"
"damage-plus-one|CAUGHT|        ((d1 * d1 + d1) / 2.0 * 7.0 * f64::from(f) + 1.0) as f32|        ((d1 * d1 + d1) / 2.0 * 7.0 * f64::from(f)) as f32"
"block-count-varint|CAUGHT|    write_i32(&mut out, block_count)?;|    write_varint(&mut out, block_count)?;"
"sound-holder-plus-one|CAUGHT|    write_varint(&mut out, sound + 1)?;|    write_varint(&mut out, sound)?;"
"step|MISSED|pub const STEP: f64 = 0.300_000_011_920_928_96;|pub const STEP: f64 = 0.3;"
"step-decay|MISSED|pub const STEP_DECAY: f32 = 0.225_000_01;|pub const STEP_DECAY: f32 = 0.225;"
"normalise-in-f32|MISSED|let d3 = (d0 * d0 + d1 * d1 + d2 * d2).sqrt();|let d3 = f64::from(((d0 * d0 + d1 * d1 + d2 * d2) as f32).sqrt());"
"ray-in-f64|MISSED|let mut d0 = f64::from((j as f32) / 15.0 * 2.0 - 1.0);|let mut d0 = f64::from(j) / 15.0 * 2.0 - 1.0;"
)

apply() {
    python3 - "$SRC" "$1" "$2" <<'PY'
import sys
path, frm, to = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(path).read()
if frm not in s:
    sys.exit(1)
open(path, 'w').write(s.replace(frm, to, 1))
PY
}

TESTS=(--test golden_explosion_ray --test golden_explosion_wire --test golden_explosion_entity)

misses=0
wrong=0
inconclusive=0
n=0
for case in "${CASES[@]}"; do
    name="${case%%|*}"; rest="${case#*|}"
    declared="${rest%%|*}"; rest="${rest#*|}"
    frm="${rest%%|*}"; to="${rest#*|}"
    n=$((n + 1))
    restore
    if ! apply "$frm" "$to"; then
        echo "PATCH MISS  $name — the mutation no longer applies to $SRC"
        misses=$((misses + 1))
        continue
    fi
    if [ "$DRYRUN" = "1" ]; then
        echo "patched     $name"
        continue
    fi
    # BUILD FIRST, AND SEPARATELY. `cargo test` exits non-zero for a failing
    # assertion AND for a build that never ran, an out-of-memory rustc, a
    # killed job and a lock it could not take — and reading all of those as
    # "the golden caught it" is this project's central defect class inside the
    # instrument that exists to refuse it. Measured 2026-09-06: this script's
    # first full run reported all four declared-MISSED cases as CAUGHT while a
    # gate was compiling on the same box, and a clean re-run of the same four
    # patches reported MISSED. Only a SUCCESSFUL build whose tests then fail
    # counts as CAUGHT; anything else is INCONCLUSIVE and stops the sweep.
    if ! PATH="$HOME/.cargo/bin:$PATH" cargo test --release \
            --manifest-path "$ROOT/Cargo.toml" -q --no-run \
            -p rustmc-server "${TESTS[@]}" >/dev/null 2>&1; then
        echo "INCONCLUSIVE $name — the mutated tree did not build. That is not"
        echo "  evidence either way: re-run when the box is quiet, and if it"
        echo "  persists the mutation is a compile error and needs rewriting."
        inconclusive=$((inconclusive + 1))
        continue
    fi
    if PATH="$HOME/.cargo/bin:$PATH" cargo test --release --manifest-path "$ROOT/Cargo.toml" \
            -q -p rustmc-server "${TESTS[@]}" >/dev/null 2>&1; then
        got=MISSED
    else
        got=CAUGHT
    fi
    if [ "$got" = "$declared" ]; then
        echo "ok   $got  $name"
    else
        echo "FAIL declared $declared, measured $got — $name"
        wrong=$((wrong + 1))
    fi
done
restore

if [ "$misses" -gt 0 ]; then
    echo "EXPLOSION-FALSIFY RED — $misses of $n mutations no longer apply, so the" >&2
    echo "sweep ran nothing for them. A patch that has stopped matching is a hole" >&2
    echo "whether or not the cases that did run were green." >&2
    exit 1
fi
if [ "$inconclusive" -gt 0 ]; then
    echo "EXPLOSION-FALSIFY INCONCLUSIVE — $inconclusive of $n cases did not build," >&2
    echo "so the sweep measured nothing for them. A build that fails for a reason" >&2
    echo "other than the mutation is indistinguishable from a caught mutation if" >&2
    echo "the two are not separated, which is why they are." >&2
    exit 3
fi
if [ "$wrong" -gt 0 ]; then
    echo "EXPLOSION-FALSIFY RED — $wrong of $n cases disagree with their declared" >&2
    echo "outcome. A CAUGHT that went MISSED is a golden that stopped seeing" >&2
    echo "something; a MISSED that went CAUGHT is good news and a stale row." >&2
    exit 1
fi
if [ "$DRYRUN" = "1" ]; then
    echo "EXPLOSION-FALSIFY DRYRUN GREEN — all $n mutations still apply, none run"
    exit 0
fi
echo "EXPLOSION-FALSIFY GREEN — $n mutations: 13 caught, 4 missed, every one as"
echo "  declared. The four missed are the precision constants and the two float"
echo "  widths; they move a ray endpoint by ~1e-7 blocks and every golden here"
echo "  records positions that have been through Mth.floor."
