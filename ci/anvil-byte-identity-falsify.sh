#!/usr/bin/env bash
# anvil-byte-identity-falsify.sh — show that
# `anvil::tests::a_real_vanilla_region_re_encodes_byte_for_byte` can go red.
#
# WHY
#
# That test compares 754 real vanilla chunks against our own re-encoding of
# them, and it was GREEN on its first run after three separate defects were
# fixed in the same commit. A green that arrives with the fix is exactly the
# shape this repo's central defect takes, so the claim "it would have caught
# them" has to be demonstrated rather than asserted. This puts each of the
# three defects back, one at a time, and requires a RED for each.
#
# It is not in `ci/t0.sh`: it edits a source file and rebuilds four times,
# which takes about a minute, and T0 already runs the test itself. Run it after
# touching `encode_chunk`, `decode_chunk` or `ChunkColumn`'s fields.
#
# WHAT EACH CASE IS
#
#   1  the heightmap offset — `Heightmap.setHeight` stores `y - getMinY()`, so
#      the decoder must add `min_y` back. Without it every saved column's
#      heightmap is a section and a half too high, and vanilla trusts a saved
#      heightmap rather than recomputing it.
#   2  `block_entities` omitted when empty — vanilla writes `[]`.
#   3  the `extra` passthrough dropped — the tick queues, `isLightOn`,
#      `InhabitedTime` and `structures` written back as constants.
#
# The control at each end is not decoration: a suite whose "red" is a build
# failure would report all three as caught while catching nothing, so the run
# has to be green before and green after.

set -u
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

F=crates/rustmc-world/src/anvil.rs
TEST=a_real_vanilla_region_re_encodes_byte_for_byte
BACKUP=$(mktemp)
cp "$F" "$BACKUP"
restore() { cp "$BACKUP" "$F"; touch "$F"; }
trap 'restore; rm -f "$BACKUP"' EXIT

fails=0
run() {
    touch "$F"
    if cargo test -p rustmc-world --lib "$TEST" 2>&1 | grep -cE '^test result: ok' >/dev/null; then
        echo GREEN
    else
        echo RED
    fi
}

expect() { # expect <wanted> <label>
    local got
    got=$(run)
    printf '%-34s %s\n' "$2" "$got"
    [ "$got" = "$1" ] || { echo "  !! wanted $1"; fails=$((fails + 1)); }
}

expect GREEN "control, unpatched:"

sed -i 's|out.push((((word >> (k \* 9)) & 0x1FF) as i32) + min_y);|out.push(((word >> (k * 9)) \& 0x1FF) as i32);|' "$F"
if grep -q 'as i32) + min_y' "$F"; then
    echo "  !! case 1 patch did not apply — the anchor has moved"
    fails=$((fails + 1))
fi
expect RED "1 heightmap offset dropped:"
restore

python3 - "$F" <<'PY'
import sys
path = sys.argv[1]
src = open(path).read()
old = '''    root.insert(
        "block_entities".into(),
        Nbt::List(chunk.block_entities.clone()),
    );'''
new = '''    if !chunk.block_entities.is_empty() {
        root.insert(
            "block_entities".into(),
            Nbt::List(chunk.block_entities.clone()),
        );
    }'''
if old not in src:
    raise SystemExit("case 2 patch did not apply - the anchor has moved")
open(path, 'w').write(src.replace(old, new, 1))
PY
[ $? -eq 0 ] || fails=$((fails + 1))
expect RED "2 empty block_entities omitted:"
restore

sed -i 's|    let mut root = chunk.extra.clone();|    let mut root = BTreeMap::new();|' "$F"
if ! grep -q 'let mut root = BTreeMap::new();' "$F"; then
    echo "  !! case 3 patch did not apply — the anchor has moved"
    fails=$((fails + 1))
fi
expect RED "3 extra passthrough dropped:"
restore

expect GREEN "restored:"

if [ "$fails" -eq 0 ]; then
    echo "ANVIL-BYTE-IDENTITY-FALSIFY GREEN  3 defects, each caught"
    exit 0
fi
echo "ANVIL-BYTE-IDENTITY-FALSIFY RED  $fails case(s) did not behave as stated"
exit 1
