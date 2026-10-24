#!/usr/bin/env bash
# movement-falsify.sh — inject known-wrong edits and check the tests catch them.
#
#     ./ci/movement-falsify.sh          # every injection
#     ./ci/movement-falsify.sh 7        # just number 7
#
# A test suite that has never been observed failing has not been shown capable
# of failing. This breaks `attributes.rs`, `movement_props.rs` and `physics.rs`
# one edit at a time, in ways a plausible reimplementation would actually get
# wrong, and reports **which injections were NOT caught**. Those are holes in
# the tests, not in the injection.
#
# Every injection is a *specific* misreading of the bytecode, not a random
# mutation: an operator flipped, a constant replaced by the one somebody would
# write from memory, a branch dropped. A sweep of arbitrary edits measures how
# brittle the code is; this measures whether the tests pin the decisions that
# were hard to read.
#
# # The substitutions are LITERAL, and the first draft of this file was not
#
# Fields are separated by `@@@` and the search text is matched under perl's
# `\Q…\E`, passed through the environment rather than the command line, so
# nothing in it is ever a metacharacter. The first version built a regex out of
# the search text and **two injections silently matched nothing**: `t += v` as
# a pattern means "t, one or more spaces, = v", because the `+` binds to the
# preceding space and the literal `+` is never matched at all.
#
# Both were reported as NOT APPLIED rather than as caught, which is the only
# reason it was noticed. **That guard is the load-bearing part of this
# script**, not the injections: an injection that never happened produces a
# green test run, and a green test run is exactly what a hole in the tests
# looks like. A sweep without the guard reports a number it has not measured.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

# Per-injection build logs. They are what the refusal below prints: a sweep
# that stops has to say what it saw, or the next reader re-runs it to find out.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

ONLY="${1:-}"
A=crates/rustmc-server/src/attributes.rs
M=crates/rustmc-server/src/movement_props.rs
P=crates/rustmc-server/src/physics.rs

# file @@@ description @@@ literal search @@@ literal replacement
INJECTIONS=(
# ---- attributes.rs: AttributeInstance.calculateValue ----
"$A@@@ADD_MULTIPLIED_BASE multiplies the running total, not the base@@@t += v * m.amount;@@@t += t * m.amount;"
"$A@@@ADD_MULTIPLIED_TOTAL adds instead of compounding@@@t *= 1.0 + m.amount;@@@t += 1.0 + m.amount;"
"$A@@@ADD_VALUE modifiers are ignored@@@            v += m.amount;@@@            let _ = m.amount;"
"$A@@@sanitizeValue maps NaN to the maximum instead of the minimum@@@            return min;@@@            return max;"
"$A@@@the clamp loses its lower bound@@@        if v < min {@@@        if false {"
"$A@@@the clamp loses its upper bound@@@        } else if v > max {@@@        } else if false {"
"$A@@@an unranged attribute is clamped as if it were ranged@@@            return v;@@@            return v.clamp(-1.0, 1.0);"
"$A@@@an empty table parses instead of being refused@@@        if specs.is_empty() || suppliers.is_empty() {@@@        if false {"

# ---- movement_props.rs ----
"$M@@@a non-living entity does not get the 0.8 bounce factor@@@            r * f64::from(0.8_f32)@@@            r"
"$M@@@the 0.8 bounce factor is applied to a living entity instead@@@        if living {@@@        if !living {"
"$M@@@friction defaults to ice for an unknown state@@@        self.row(state_id).map_or(0.6, |r| r.friction)@@@        self.row(state_id).map_or(0.98, |r| r.friction)"
"$M@@@speed_factor defaults to zero for an unknown state@@@        self.row(state_id).map_or(1.0, |r| r.speed_factor)@@@        self.row(state_id).map_or(0.0, |r| r.speed_factor)"
"$M@@@an out-of-range state row is accepted@@@            if i >= rows.len() {@@@            if false {"
"$M@@@is_fence defaults true for an unknown state@@@        self.row(state_id).is_some_and(|r| r.fence)@@@        self.row(state_id).is_none_or(|r| r.fence)"
"$M@@@is_fence reads the wall column@@@        self.row(state_id).is_some_and(|r| r.fence)@@@        self.row(state_id).is_some_and(|r| r.wall_or_gate)"
"$M@@@is_wall_or_gate reads the fence column@@@        self.row(state_id).is_some_and(|r| r.wall_or_gate)@@@        self.row(state_id).is_some_and(|r| r.fence)"
"$M@@@an empty table parses instead of being refused@@@        if rows.is_empty() || state_rows.is_empty() {@@@        if false {"

# ---- physics.rs ----
"$P@@@Mth.equal uses the f64 epsilon instead of the widened f32@@@    (b - a).abs() < f64::from(1.0E-5_f32)@@@    (b - a).abs() < 1.0E-5_f64"
"$P@@@Mth.equal is inclusive at the boundary@@@    (b - a).abs() < f64::from(1.0E-5_f32)@@@    (b - a).abs() <= f64::from(1.0E-5_f32)"
"$P@@@gravity is added rather than subtracted@@@body.movement.y - gravity@@@body.movement.y + gravity"
"$P@@@gravity is applied unconditionally, losing the isNoGravity fold@@@    if gravity != 0.0 {@@@    if true {"
"$P@@@item gravity is 0.08, the number everyone remembers@@@        gravity: 0.04,@@@        gravity: 0.08,"
"$P@@@the bounding box does not halve the width@@@        let w = self.width / 2.0;@@@        let w = self.width;"
"$P@@@an unknown entity kind gets an item profile@@@            _ => None,@@@            _ => Some(Self::ITEM),"
"$P@@@BlockPos.compareTo is x, then y, then z@@@    a.y.cmp(&b.y).then(a.z.cmp(&b.z)).then(a.x.cmp(&b.x))@@@    a.x.cmp(&b.x).then(a.y.cmp(&b.y)).then(a.z.cmp(&b.z))"
"$P@@@findSupportingBlock keeps the smaller BlockPos on a tie@@@std::cmp::Ordering::Less)@@@std::cmp::Ordering::Greater)"
"$P@@@distToCenterSqr uses the block corner rather than its centre@@@        let dx = f64::from(p.x) + 0.5 - pos.x;@@@        let dx = f64::from(p.x) - pos.x;"
"$P@@@getOnPosLegacy uses the movement offset instead of 0.2@@@    on_pos(body, 0.2_f32, is_fence, is_wall_or_gate)@@@    on_pos(body, 0.500_001_f32, is_fence, is_wall_or_gate)"
"$P@@@getBlockPosBelowThatAffectsMyMovement uses a plain 0.5@@@    on_pos(body, 0.500_001_f32, is_fence, is_wall_or_gate)@@@    on_pos(body, 0.5_f32, is_fence, is_wall_or_gate)"
"$P@@@the fence arm loses its offset <= 0.5 guard@@@            if (offset <= 0.5 && is_fence(p)) || is_wall_or_gate(p) {@@@            if is_fence(p) || is_wall_or_gate(p) {"
"$P@@@a wall gains an offset guard it does not have@@@            if (offset <= 0.5 && is_fence(p)) || is_wall_or_gate(p) {@@@            if offset <= 0.5 && (is_fence(p) || is_wall_or_gate(p)) {"
"$P@@@the tiny-offset early return is dropped@@@            if offset <= 1.0E-5_f32 {@@@            if false {"
"$P@@@getOnPos keeps the entity column rather than the support column@@@                x: p.x,@@@                x: body.pos.x.floor() as i32,"
"$P@@@checkSupportingBlock skips the backwards-shifted second probe@@@        let back = probe.shifted(-m.x, 0.0, -m.z);@@@        let back = probe;"
"$P@@@the supporting-block probe box is a whole block deep@@@bb.min_y - 1.0E-6@@@bb.min_y - 1.0"
"$P@@@verticalCollisionBelow drops its downward test@@@body.vertical_collision && vec.y < 0.0;@@@body.vertical_collision;"
"$P@@@the ground flag updates on a purely horizontal move@@@    let moved_vertically = vec.y.abs() > 0.0;@@@    let moved_vertically = true;"
"$P@@@horizontalCollision uses != instead of Mth.equal@@@    let x_collided = !mth_equal(vec.x, moved.x);@@@    let x_collided = vec.x != moved.x;"
"$P@@@verticalCollision uses Mth.equal instead of !=@@@        body.vertical_collision = vec.y != moved.y;@@@        body.vertical_collision = !mth_equal(vec.y, moved.y);"
"$P@@@the block speed factor multiplies y as well as x and z@@@body.movement.x * f, body.movement.y, body.movement.z * f@@@body.movement.x * f, body.movement.y * f, body.movement.z * f"
"$P@@@getBlockSpeedFactor loses its water short circuit@@@    if props.is_water_or_bubble_column(id) {@@@    if false {"
"$P@@@getBlockSpeedFactor falls back even when the factor is not 1@@@    if f == 1.0 {@@@    if true {"
"$P@@@the piston mover falls through to the normal path@@@    if mover == MoverType::Piston {@@@    if false {"
"$P@@@the setPos guard loses its second disjunct@@@    if moved_len > 1.0E-7 || vec.length_sqr() - moved_len < 1.0E-7 {@@@    if moved_len > 1.0E-7 {"
"$P@@@Mth.lerp is written as a plain multiply@@@(r * effective_gravity, 1.0 + r * (drag - 1.0))@@@(r * effective_gravity, r * drag)"
)

pass=0
uncaught=()
notapplied=()
i=0
for inj in "${INJECTIONS[@]}"; do
    i=$((i + 1))
    [ -n "$ONLY" ] && [ "$ONLY" != "$i" ] && continue
    file="${inj%%@@@*}"
    r1="${inj#*@@@}"
    desc="${r1%%@@@*}"
    r2="${r1#*@@@}"
    search="${r2%%@@@*}"
    replace="${r2#*@@@}"

    cp "$file" "$file.orig"
    # Literal, through the environment: nothing in `search` is ever a
    # metacharacter and nothing in `replace` is ever a backreference.
    S="$search" R="$replace" perl -0777 -pe 's/\Q$ENV{S}\E/$ENV{R}/' "$file.orig" > "$file"

    if cmp -s "$file" "$file.orig"; then
        echo "  !! $i NOT APPLIED  $desc"
        notapplied+=("$i: $desc")
        mv "$file.orig" "$file"
        continue
    fi

    # `--lib` only: these three modules are unit-tested in place and the
    # integration targets take minutes. If a future injection lands in a
    # module an integration test owns, widen this and say so.
    #
    # **Debug, not release, and that is a deliberate difference from the
    # gate.** Every injection recompiles `rustmc-server`; in release that is
    # minutes each and forty-odd of them is most of an afternoon, which is how
    # a sweep stops being run. Nothing here depends on the profile: these are
    # `f64` and `f32` comparisons, Rust does not reassociate or contract them
    # at any optimisation level, and none of the injected code can overflow an
    # integer — the one thing debug and release genuinely disagree about.
    # **`else` means "the tests failed", and it has to be made to mean only
    # that.** Anything that stops `cargo test` from reaching a verdict lands
    # here too and scores as a catch: a missing cargo (127 — and a
    # non-interactive `ssh` has no `~/.cargo/bin` on its PATH, which is exactly
    # how this class of thing bites on this project), a compile error in an
    # injection, a full disk. The sweep would then report every injection
    # caught having compiled nothing, and it would be a perfect score. That is
    # the project's signature defect — a green that means nothing — inside the
    # instrument built to find it. Refuse rather than score.
    cargo test -q -p rustmc-server --lib > "$WORK/inject-$i.log" 2>&1
    trc=$?
    if [ "$trc" -eq 0 ]; then
        echo "  UNCAUGHT $i  $desc"
        uncaught+=("$i: $desc")
    elif [ "$trc" -eq 101 ]; then       # cargo's "the tests ran and failed"
        echo "  caught   $i  $desc"
        pass=$((pass + 1))
    else
        mv "$file.orig" "$file"
        echo "movement-falsify: REFUSED — injection $i did not reach a test verdict" >&2
        echo "    cargo test exited $trc, which is neither pass (0) nor test-failure (101)." >&2
        echo "    A sweep cannot score an injection it never compiled. Last 20 lines:" >&2
        tail -20 "$WORK/inject-$i.log" >&2
        exit 2
    fi
    mv "$file.orig" "$file"
done

echo
echo "falsification: $pass caught, ${#uncaught[@]} NOT caught, ${#notapplied[@]} not applied, of ${#INJECTIONS[@]} written"
if [ ${#uncaught[@]} -gt 0 ]; then
    echo "holes (the tests do not pin these):"
    for u in "${uncaught[@]}"; do echo "  $u"; done
fi
if [ ${#notapplied[@]} -gt 0 ]; then
    echo "NOT APPLIED (broken injections — neither caught nor a hole, fix them):"
    for u in "${notapplied[@]}"; do echo "  $u"; done
fi
[ ${#notapplied[@]} -eq 0 ] || exit 2
