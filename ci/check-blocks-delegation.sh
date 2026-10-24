#!/usr/bin/env bash
# check-blocks-delegation.sh — every defaulted `Blocks` method is delegated by
# `Tiles`, the production world.
#
# # The defect this exists for, and why nothing else catches it
#
# `crate::edits::Blocks` is the world as every ported mechanic sees it. Most
# of its methods are required, so a new one breaks every impl until it is
# written — which is a compiler error and needs no gate.
#
# A handful have **default bodies**, and those are different in kind. The
# default exists because the great majority of `Blocks` impls in this crate
# are test fakes with no `block_props.json` and no height model behind them,
# and making all of them answer a question they have no opinion on would be
# inventing the answer in ten places instead of one. So the default returns
# `None`, and every caller reads `None` as "the table cannot answer, do
# nothing".
#
# Put those two facts together and you get a port that cannot run:
#
#   * `crate::tiles::Tiles` is the production world and delegates every read
#     to the tile that owns the position;
#   * a defaulted method it does *not* delegate silently takes the trait
#     default;
#   * the caller sees `None` and does nothing;
#   * and **doing nothing is indistinguishable from the mechanism not having
#     fired**, which is what a correct server looks like most of the time.
#
# Meanwhile every unit test passes, because the fakes those tests run against
# override the very method the real world drops. This is the project's central
# defect class — a green whose red cannot be produced — arriving through the
# type system rather than through a check.
#
# **Two instances shipped this way and T0 was green on both.** They are named
# in `crates/rustmc-server/src/tiles.rs` beside the delegations that fix them:
# `level_min_y`, without which `FallingBlock.tick`'s whole arm is skipped and
# sand does not fall on a real server; and `note_block_instrument`, without
# which a note block is placed with the golden harness's instrument rather
# than the ground's. Both were found by reading `Tiles`, not by any gate.
#
# # What it does
#
# Parses the defaulted method names out of the trait — a signature whose line
# ends in `{` rather than `;` — and requires each to appear as an `fn` inside
# `impl Blocks for Tiles`. It reads the impl block by its own boundaries
# rather than the whole file, so a method that merely exists on `Tiles`
# somewhere else does not satisfy it.
#
# Exits 1 naming every missing one. Its own red-when-broken evidence is
# `--falsify`, which removes each delegation from a copy of the file in turn
# and requires this script to refuse it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EDITS="$ROOT/crates/rustmc-server/src/edits.rs"
# Resolved per call rather than once, so `--falsify` can point the checker at
# a mutated copy. Binding it at the top made every mutation invisible and the
# falsify sweep caught that on its first run — which is the sweep earning its
# place before it ever guarded the real thing.
tiles_file() { echo "${TILES_FILE:-$ROOT/crates/rustmc-server/src/tiles.rs}"; }

defaulted() {
    # A trait method with a body: `fn name(...) -> T {` at the trait's
    # indentation. `;` is a required method and is the compiler's problem.
    sed -n '/^pub trait Blocks/,/^}/p' "$EDITS" \
        | grep -oP '^    fn \K[a-z_0-9]+(?=\(.*\{\s*$)' | sort -u
}

# The `impl Blocks for Tiles` block, by its own braces.
impl_block() {
    awk '/^impl Blocks for Tiles \{/{f=1} f{print} f&&/^\}$/{exit}' "$(tiles_file)"
}

check() {
    local missing=0 name
    local impl
    impl="$(impl_block)"
    if [ -z "$impl" ]; then
        echo "check-blocks-delegation: no 'impl Blocks for Tiles' in $(tiles_file)" >&2
        return 1
    fi
    local total=0
    for name in $(defaulted); do
        total=$((total + 1))
        if ! grep -qE "^    fn $name\b" <<<"$impl"; then
            echo "check-blocks-delegation: Tiles does not delegate '$name'" >&2
            echo "  It is a DEFAULTED method on Blocks, so this compiles and returns None," >&2
            echo "  and every caller reads None as 'do nothing' — which is what a correct" >&2
            echo "  server looks like. Delegate it in crates/rustmc-server/src/tiles.rs." >&2
            missing=$((missing + 1))
        fi
    done
    if [ "$missing" -gt 0 ]; then
        return 1
    fi
    echo "check-blocks-delegation: $total defaulted Blocks methods, all delegated by Tiles"
}

falsify() {
    local work caught=0 tried=0 name
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' RETURN
    for name in $(defaulted); do
        tried=$((tried + 1))
        # Delete the delegation's signature line, which is enough to make the
        # method fall back to the trait default.
        sed "/^    fn $name\b/d" "$(TILES_FILE= tiles_file)" > "$work/tiles.rs"
        if TILES_FILE="$work/tiles.rs" check >/dev/null 2>&1; then
            echo "  NOT CAUGHT  removing $name's delegation left this script green" >&2
        else
            echo "  caught      $name"
            caught=$((caught + 1))
        fi
    done
    echo "check-blocks-delegation --falsify: caught $caught of $tried"
    [ "$caught" -eq "$tried" ]
}

if [ "${1:-}" = "--falsify" ]; then
    falsify
else
    check
fi
