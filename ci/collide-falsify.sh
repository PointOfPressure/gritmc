#!/usr/bin/env bash
# collide-falsify.sh — break `collide.rs` on purpose, one bug at a time, and
# report every bug the gate did NOT catch.
#
#     ./ci/collide-falsify.sh                 # all injections
#     ./ci/collide-falsify.sh <name>          # just one
#
# `crates/rustmc-server/tests/entity_move.rs` went green on its first run over
# 2,484 vanilla cases. Rule 7 of `docs/AGENT-BRIEF.md`: when a measurement
# clears every candidate, suspect the measurement. The last three agents on
# this project each found a hole in their OWN tests this way — one had a
# `grep -F` guard that matched any line and so could never fail — so the
# assumption here is that at least one hole exists and the job is to find it.
#
# Each injection is a transcription error a careful reader could plausibly
# make, applied to the real source, gated, and reverted. An injection that
# leaves the suite GREEN is a hole in the suite and is reported as such;
# the script exits non-zero if any does.
#
# This runs on the Threadripper like everything else. It rsyncs the working
# tree per injection, so it must not be run concurrently with anything else
# using the same `RUSTMC_REMOTE_DIR`.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
HOST="${RUSTMC_REMOTE:-threadripper}"
. "$(dirname "$0")/lib/require-remote-dir.sh"
DEST="$RUSTMC_REMOTE_DIR"
SRC="crates/rustmc-server/src/collide.rs"
ONLY="${1:-}"

# name | old | new | expect
#
# The separator is a tab so that the patterns can contain anything but one.
#
# `expect` is the point of the fourth column and it is not documentation:
#
#   caught         the gate must go red. If it does not, the gate has a hole.
#   unobservable   the injected change **cannot** alter `collide`'s output,
#                  for a reason written down below. If the gate goes RED on
#                  one of these, the reason is wrong and that is as much a
#                  finding as a hole. Either direction fails this script.
#   open           not caught, and no argument for why it cannot be. A known
#                  coverage hole, reported every run rather than forgotten.
#
# The `unobservable` arguments, each of which this script will falsify if it
# is wrong:
#
#   mth_epsilon_is_double     the two constants differ by 2.5e-13. The only
#     thing that value reaches is which blocks join the step-up collider list,
#     and every test that list is then put through -- `span_overlaps` and
#     `collideX`'s four cross-axis `findIndex` calls -- carries its own 1e-7
#     margin, which is five orders of magnitude larger. A block admitted or
#     refused by 2.5e-13 is ignored by the sweep either way.
#
#   step_start_y_from_delta   `startY` filters candidate step heights, and a
#   step_heights_keep_start   candidate can only improve the result by lifting
#     the box HIGHER than the y sweep already put it, i.e. h > resolved.y. But
#     `collideWithShapes` re-runs the y sweep for the candidate, so h is only
#     reachable when h <= the headroom -- and when y was clamped upward the
#     headroom IS resolved.y. So no h can be both reachable and an
#     improvement, and which candidates are in the list cannot matter. When y
#     was clamped downward, resolved.y < 0 <= h for every candidate, so the
#     filter never fires. When y was not clamped, resolved.y == v.y and the two
#     spellings are the same expression.
#
#   offset_moves_the_box      `(c + o) - h` against `c - (h - o)`, where o is
#   collide_t_regrouped       an integer block coordinate. Inside the world
#     border |o| < 2^25, and shape coordinates are multiples of 1/8, so `c + o`
#     needs at most 28 bits and is exact. A block only collides with a box
#     adjacent to it, so o/2 <= h <= 2o and `h - o` is exact by Sterbenz.
#     Both spellings therefore compute the same real number exactly. It is
#     transcribed vanilla's way regardless, because the argument stops holding
#     the moment either premise does.
#
#   step_heights_continue     `getCoords` is ascending, so once one height
#     exceeds `maxUpStep` every later one does too. `break` and `continue` skip
#     the same set; the `break` is an optimisation.
#
#   face_cells_always         a face cell lies outside the query box by
#     construction -- the cursor spans floor(min)-1 to floor(max)+1 -- so a
#     shape confined to its own cell cannot reach the box. The
#     `hasLargeCollisionShape` filter can only skip blocks that would have
#     contributed nothing.
#
#   expand_towards_else       `dx > 0` to `dx >= 0` only changes the dx == 0
#     case, and `x += 0.0` is a no-op for every double except -0.0.
#
# The one `open` entry, with the hypothesis that did NOT get promoted to an
# argument, written down so the next person starts where this one stopped:
#
#   grounded_ignores_direction  dropping the `v.y < 0.0` test only matters when
#     y was clamped UPWARD, and the argument for why that cannot show is:
#     `resolved` computed its horizontal from the box raised by the headroom r,
#     the step box in the buggy branch is raised by exactly r and so has zero
#     headroom left, and in the correct branch every candidate resolves to a
#     height min(h, r) <= r, from which the box can only be MORE blocked. So
#     `stepped.horizontalDistanceSqr() > resolved...` should never hold either
#     way. It is left `open` rather than `unobservable` because that chain has
#     one weak link: the two branches collect their colliders from differently
#     expanded boxes, and a block present in one list and not the other could
#     in principle make the stepped sweep LESS blocked. No construction of that
#     was found; no proof that none exists was found either.
#
#   zero_axis_not_skipped     removing the `d == 0.0` skip makes
#     `shapes_collide` run with d = 0.0, whose first act is to return 0.0 for
#     `abs(d) < EPS`; `acc.with(axis, 0.0)` then writes the value already
#     there.
INJECTIONS=$(cat <<'TSV'
axis_order_fixed	if v.x.abs() < v.z.abs() {	if false {	caught
axis_order_flipped	if v.x.abs() < v.z.abs() {	if v.x.abs() > v.z.abs() {	caught
mth_epsilon_is_double	const MTH_EQUAL_EPSILON: f64 = -(1.0E-5_f32 as f64);	const MTH_EQUAL_EPSILON: f64 = -1.0E-5;	unobservable
step_start_y_from_delta	let start_y = resolved.y as f32;	let start_y = v.y as f32;	unobservable
step_box_from_delta	box_.shifted(0.0, resolved.y, 0.0)	box_.shifted(0.0, v.y, 0.0)	caught
cursor_order_reversed	for z in z0..=z1 {\n        for y in y0..=y1 {\n            for x in x0..=x1 {	for x in x0..=x1 {\n        for y in y0..=y1 {\n            for z in z0..=z1 {	caught
admission_strict	box_min < cell_max - EPS && cell_min < box_max - EPS	box_min < cell_max && cell_min < box_max	caught
full_block_uses_overlap	if box_.intersects_cell(x, y, z) {	if shape.overlaps(box_, [f64::from(x), f64::from(y), f64::from(z)]) {	caught
offset_moves_the_box	if v < coords[mid] + off {	if v - off < coords[mid] {	unobservable
collide_t_regrouped	let t = (self.coords[a][i as usize] + off[a]) - hi;	let t = self.coords[a][i as usize] - (hi - off[a]);	unobservable
shapes_collide_eps_after	if d.abs() < EPS {\n            return 0.0;\n        }\n        d = s.collide(axis, box_, d);	d = s.collide(axis, box_, d);\n        if d.abs() < EPS {\n            return 0.0;\n        }	caught
step_heights_continue	if h > max_up_step {\n                break;	if h > max_up_step {\n                continue;	unobservable
step_heights_keep_start	if h == start_y {\n                continue;\n            }	if h == start_y && false {\n                continue;\n            }	unobservable
grid_rotation_identity	1 => (k, i, j),\n            _ => (j, k, i),	1 => (i, j, k),\n            _ => (i, j, k),	caught
grounded_ignores_direction	let grounded = y_changed && v.y < 0.0;	let grounded = y_changed;	open
step_up_ge_not_gt	if stepped.horizontal_distance_sqr() > resolved.horizontal_distance_sqr() {	if stepped.horizontal_distance_sqr() >= resolved.horizontal_distance_sqr() {	caught
face_cells_always	if cell_type == 1 && !shape.is_large() {	if cell_type == 1 && false {	unobservable
expand_towards_else	if dx > 0.0 {\n            b.max_x += dx;\n        }	if dx >= 0.0 {\n            b.max_x += dx;\n        }	unobservable
floor_truncates	let f = v.floor();\n    f as i32	v as i32	caught
shell_not_expanded	let x0 = floor(box_.min_x - EPS) - 1;	let x0 = floor(box_.min_x - EPS);	caught
zero_axis_not_skipped	if d == 0.0 {\n            continue;\n        }	if d == 0.0 && false {\n            continue;\n        }	unobservable
step_uses_original_box	collide_with_shapes(Vec3::new(v.x, f64::from(h), v.z), step_box, &colliders);	collide_with_shapes(Vec3::new(v.x, f64::from(h), v.z), box_, &colliders);	caught
corner_cells_included	if cell_type == 3 {\n                    continue;\n                }	if cell_type == 4 {\n                    continue;\n                }	caught
edge_cells_included	if cell_type == 2 {	if cell_type == 99 {	caught
TSV
)

cp "$SRC" "/tmp/collide-falsify-pristine.rs" || exit 2
restore() { cp "/tmp/collide-falsify-pristine.rs" "$SRC"; }
trap restore EXIT

HOLES=()
WRONG_ARGUMENT=()
OPEN=()
CAUGHT=0
SKIPPED=0

echo "== falsification sweep on $SRC against $DEST"

while IFS=$'\t' read -r NAME OLD NEW EXPECT; do
    [ -n "$NAME" ] || continue
    [ -z "$ONLY" ] || [ "$ONLY" = "$NAME" ] || continue
    restore
    # `printf %b` expands the \n in the table; python does the replacement so
    # that the patterns are literal and not regexes.
    if ! OLD_TEXT="$(printf '%b' "$OLD")" || ! NEW_TEXT="$(printf '%b' "$NEW")"; then
        echo "  ?? $NAME — could not expand its patterns" >&2
        SKIPPED=$((SKIPPED + 1)); continue
    fi
    OLD_TEXT="$OLD_TEXT" NEW_TEXT="$NEW_TEXT" python3 - "$SRC" <<'PY'
import os, sys
path = sys.argv[1]
old = os.environ["OLD_TEXT"]
new = os.environ["NEW_TEXT"]
s = open(path).read()
n = s.count(old)
if n != 1:
    sys.exit(3 if n == 0 else 4)
open(path, "w").write(s.replace(old, new))
PY
    RC=$?
    if [ "$RC" = 3 ]; then
        # **An injection that does not apply is not a pass.** It is the same
        # class of fault as a guard that matches any line: the sweep would
        # report "caught" for a bug it never introduced.
        echo "  !! $NAME — its pattern is not in the source (0 matches); the "\
             "injection is stale, not caught" >&2
        SKIPPED=$((SKIPPED + 1)); continue
    fi
    if [ "$RC" = 4 ]; then
        echo "  !! $NAME — its pattern matches more than once; ambiguous" >&2
        SKIPPED=$((SKIPPED + 1)); continue
    fi

    rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
        --exclude 'data/vanilla' "$ROOT/" "$HOST:$DEST/" >/dev/null 2>&1
    OUT="$(ssh -n "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; cd $DEST && \
        cargo test -q -p rustmc-server --test entity_move 2>&1")"
    TRC=$?
    if echo "$OUT" | grep -c 'error\[E\|error: could not compile' >/dev/null; then
        # A bug that will not compile is not evidence about the gate. Said out
        # loud rather than counted as caught.
        echo "  -- $NAME  DID NOT COMPILE (no evidence either way)"
        SKIPPED=$((SKIPPED + 1)); continue
    fi
    if [ "$TRC" != 0 ]; then
        CAUGHT=$((CAUGHT + 1))
        if [ "$EXPECT" = unobservable ]; then
            echo "  !! $NAME  CAUGHT, but it is recorded as unobservable — the"\
                 "argument in this file's header is WRONG"
            WRONG_ARGUMENT+=("$NAME")
        else
            echo "  ok $NAME  CAUGHT"
        fi
    else
        case "$EXPECT" in
            caught)
                echo "  !! $NAME  NOT CAUGHT — the suite is green with this bug in it"
                HOLES+=("$NAME") ;;
            unobservable)
                echo "  .. $NAME  not caught, and cannot be — see the header" ;;
            open)
                echo "  ?? $NAME  NOT CAUGHT and unexplained — a known hole"
                OPEN+=("$NAME") ;;
            *)
                echo "  !! $NAME  has no expectation in the table" >&2
                HOLES+=("$NAME") ;;
        esac
    fi
done <<< "$INJECTIONS"

restore
echo
echo "falsification: $CAUGHT caught, ${#HOLES[@]} holes, ${#WRONG_ARGUMENT[@]} wrong"\
     "arguments, ${#OPEN[@]} open, $SKIPPED skipped"
RC=0
if [ "${#HOLES[@]}" != 0 ]; then
    echo "HOLES — expected to be caught and were not:"
    for n in "${HOLES[@]}"; do echo "  - $n"; done
    RC=1
fi
if [ "${#WRONG_ARGUMENT[@]}" != 0 ]; then
    echo "WRONG ARGUMENTS — recorded as unobservable and the gate caught them:"
    for n in "${WRONG_ARGUMENT[@]}"; do echo "  - $n"; done
    RC=1
fi
if [ "${#OPEN[@]}" != 0 ]; then
    echo "OPEN — not caught and not explained. Reported every run, deliberately:"
    for n in "${OPEN[@]}"; do echo "  - $n"; done
fi
[ "$CAUGHT" -ge 12 ] || {
    echo "only $CAUGHT injections went red — the sweep is too thin to conclude" >&2
    RC=1
}
exit "$RC"
