#!/usr/bin/env bash
# be-channel-falsify.sh — proof that the block-entity channel's guards can go
# red, by reconstructing the two defects they exist to catch.
#
# `docs/AGENT-BRIEF.md`: *a guard that cannot be shown catching a real past
# instance is itself a check that cannot fail.* The channel in
# `docs/DETERMINISM.md` §42 is guarded by two instruments, and they catch
# **different** things — which is the whole reason both exist and the reason
# this script injects two defects rather than one:
#
#   INJECTION 1 — a read path back into the world.
#     `Canvas::record_block_entity` also writes a block. This is what a
#     careless "mark the position while I am here" implementation looks like,
#     and it is caught by `ci/be-channel-identical.sh`, because the recording
#     arm's world stops matching the plain arm's.
#
#   INJECTION 2 — a payload built from a fresh draw.
#     The producer takes an extra `nextLong` to fill the record in. This is
#     the more likely mistake and **the identity gate cannot see it**: the
#     draw is spent in both arms, so both worlds move together and the two
#     agree perfectly while both are wrong. It is caught by the draw-count
#     unit test in `feature::monster_room`, which compares the feature's draws
#     against an independent replay of the same stream.
#
# The pair is the point. An agent who read only the §42.5 gate would conclude
# that on/off identity is the whole condition; it is half of it, and the other
# half is that the channel spends no draw at all. Both injections are applied
# to a scratch copy of the tree and the originals are never touched.
#
# Runtime is dominated by two release builds. `BE_FALSIFY_DRYRUN=1` applies
# both patches, checks they still match, and runs nothing — do that after
# touching any of the files below, because a patch that has stopped matching
# is a hole in the sweep whether or not you have the minutes.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The scratch tree is NESTED one level inside its own root, and that is not
# cosmetic. The workspace has an out-of-tree member
# (`grit-plugin-api = { path = "../plugins/..." }`), so something must resolve
# BESIDE the staged tree. With `WORK` directly in `/tmp` that sibling is
# `/tmp/plugins` — a name shared with every other process on the box, which two
# concurrent runs of this script would fight over. One level down it is private
# to this run and disappears with it.
WORKROOT=${BE_FALSIFY_WORK:-/tmp/be-channel-falsify.$$}
WORK="$WORKROOT/tree"
# A shared target directory, because three release builds from scratch is the
# difference between a falsifier that gets run and one that does not.
#
# It is a SYMLINK at `$WORK/target` and deliberately not `CARGO_TARGET_DIR`.
# Setting that variable puts the binaries somewhere `ci/be-channel-identical.sh`
# does not look — it reads `$ROOT/target/release/...` — so the gate refuses for a
# missing binary, that refusal is indistinguishable here from the gate catching
# the injection, and every verdict in this script becomes unattributable. Which
# is exactly what happened the first time it was run that way, and what the
# clean-tree control at the bottom exists to catch.
TARGET=${BE_FALSIFY_TARGET:-}
DRY=${BE_FALSIFY_DRYRUN:-0}
rc=0

CANVAS=crates/rustmc-worldgen/src/canvas.rs
ROOM=crates/rustmc-worldgen/src/feature/monster_room.rs

# The anchors each injection needs. Kept here so a rename shows up as a
# refusal rather than as a silently skipped case.
ANCHOR_1='        if let Some(records) = self.block_entities.as_mut() {'
ANCHOR_2='                let loot_seed = random.next_i64();'

echo "=== staging a scratch copy of the tree in $WORK ==="
mkdir -p "$WORK" || exit 2
git -C "$ROOT" archive HEAD | tar -x -C "$WORK" || {
    echo "REFUSED  could not stage the tree — is HEAD committed?"
    exit 2
}
# The scratch tree needs the same gitignored inputs the gate reads, and it needs
# them re-staged after EVERY `git archive` extract below.
#
# `rm -rf` before every `ln -sfn`, and that is not belt-and-braces. `data/`
# holds one tracked file, `data/README.md`, so `git archive HEAD | tar -x`
# creates a real `data` DIRECTORY — and `ln -sfn /a/b /existing/dir` then puts
# the link INSIDE it, as `dir/b`. The result is a scratch tree with
# `data/data -> .../data` and no `data/vanilla` at all, so every arm dies with
# `worldgen-dump: no block-props extract` and every verdict in this script is
# unattributable. Measured: that is exactly what the first shared-target run
# did, and the clean-tree control at the bottom is what surfaced it. Same family
# as the trailing-slash rsync that once replaced a remote `oracle-work` with a
# self-pointing symlink.
#
# The extract path is baked in at COMPILE time — `state_spec::extract_or_skip`
# resolves `CARGO_MANIFEST_DIR/../../data/vanilla/block_props.json` — so it is
# `$WORK`'s own `data` that has to resolve, not `$ROOT`'s.
stage_inputs() {
    rm -rf "$WORK/oracle-work" "$WORK/data"
    ln -sfn "$ROOT/oracle-work" "$WORK/oracle-work"
    ln -sfn "$ROOT/data" "$WORK/data"
    # The workspace reaches OUTSIDE its own root for one member:
    # `grit-plugin-api = { path = "../plugins/crates/grit-plugin-api" }`. In a
    # scratch tree that resolves to `$(dirname $WORK)/plugins`, which is
    # `/tmp/plugins` and does not exist, so `cargo` cannot even load the
    # manifest and EVERY arm of this script dies before the injection is
    # tested. Measured 2026-08-18: injection 1 and the clean-tree control both
    # came back REFUSED with `failed to read /tmp/plugins/.../Cargo.toml`,
    # which is the control doing its job — the reds were unattributable and it
    # said so.
    #
    # A sibling symlink rather than a copy: it is a build input, read-only
    # here, and copying it would make this script the second place that
    # decides what version of it the tree builds against.
    rm -rf "$WORKROOT/plugins"
    ln -sfn "$ROOT/../plugins" "$WORKROOT/plugins"
    [ -e "$WORKROOT/plugins/crates/grit-plugin-api/Cargo.toml" ] || {
        echo "REFUSED  the workspace's out-of-tree member does not resolve beside"
        echo "         $WORK — cargo would fail to load the manifest and every"
        echo "         arm would die for that reason rather than the injection."
        return 1
    }
    if [ -n "$TARGET" ]; then
        mkdir -p "$TARGET"
        rm -rf "$WORK/target"
        ln -sfn "$TARGET" "$WORK/target"
    fi
    [ -e "$WORK/data/vanilla/block_props.json" ] || {
        echo "REFUSED  $WORK/data/vanilla/block_props.json does not resolve — the"
        echo "         scratch tree has no block-props extract and every arm would"
        echo "         die for that reason rather than for the injection."
        return 1
    }
    return 0
}
stage_inputs || { rm -rf "$WORKROOT"; exit 2; }
[ -n "$TARGET" ] && echo "sharing a target directory: $TARGET"

check_anchor() {
    local file=$1 anchor=$2 name=$3
    if ! grep -qF "$anchor" "$WORK/$file"; then
        echo "REFUSED  $name: its anchor is no longer in $file — this case is a"
        echo "         hole in the sweep, not a pass. Re-derive the anchor."
        return 1
    fi
    return 0
}

inject_1() {
    python3 - "$WORK/$CANVAS" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
anchor = "        if let Some(records) = self.block_entities.as_mut() {"
assert s.count(anchor) == 1, s.count(anchor)
s = s.replace(anchor, """        if self.block_entities.is_some() {
            // INJECTED DEFECT: the channel writing back into the world.
            //
            // y + 1, NOT y. Writing air at the recorded position destroys the
            // block that was just placed, so the recording arm's census comes
            // back empty and the gate refuses as VACUOUS — a refusal by the
            // positive control rather than by the block-state comparison this
            // case is meant to exercise. The claim would then be caught for
            // the wrong reason, which is worth as little here as a green for
            // the wrong reason.
            self.set_block(x, y + 1, z, "minecraft:air");
        }
""" + anchor, 1)
open(p, "w").write(s)
PY
}

inject_2() {
    python3 - "$WORK/$ROOM" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read()
anchor = "                let loot_seed = random.next_i64();"
assert s.count(anchor) == 1, s.count(anchor)
# INJECTED DEFECT: a second draw to fill the payload in. Spent in both arms,
# so the identity gate is blind to it by construction.
s = s.replace(anchor, anchor + "\n                let _extra = random.next_i64();", 1)
open(p, "w").write(s)
PY
}

check_anchor "$CANVAS" "$ANCHOR_1" "injection 1 (channel writes a block)" || rc=1
check_anchor "$ROOM" "$ANCHOR_2" "injection 2 (payload spends a draw)" || rc=1
[ "$rc" -ne 0 ] && { rm -rf "$WORKROOT"; exit 1; }

if [ "$DRY" = 1 ]; then
    inject_1 && echo "injection 1 applies"
    inject_2 && echo "injection 2 applies"
    rm -rf "$WORKROOT"
    echo "DRYRUN  both patches still match; no case was run"
    exit 0
fi

export PATH="$HOME/.cargo/bin:$PATH"

# --- injection 2 first: it is the cheap one ----------------------------------
echo "=== injection 2 — the producer spends an extra draw ==="
inject_2
if (cd "$WORK" && cargo test --release -p rustmc-worldgen --lib monster_room \
        > "$WORK/inj2.log" 2>&1); then
    echo "REFUSED  the draw-count unit tests PASSED with an extra nextLong in"
    echo "         the producer. They are not measuring the draw count."
    rc=1
else
    echo "  caught: monster_room's draw tests went red, as they must"
fi
git -C "$ROOT" archive HEAD -- "$ROOM" | tar -x -C "$WORK"
stage_inputs || exit 2

# ...and the same defect must be INVISIBLE to the identity gate, which is the
# claim this script exists to make rather than assert.
echo "=== injection 2 against the identity gate (expected: green, i.e. blind) ==="
inject_2
if (cd "$WORK" && cargo build --release -p worldgen-dump -p region-differ \
        > "$WORK/build2.log" 2>&1) && (cd "$WORK" && ./ci/be-channel-identical.sh \
        > "$WORK/gate2.log" 2>&1); then
    echo "  confirmed: the identity gate is green on a defect it cannot see."
    echo "             That is why injection 2 has a unit test of its own."
else
    echo "  NOTE the identity gate went red on injection 2 as well. That is not"
    echo "       a failure of this script — it is a stronger gate than claimed."
    tail -3 "$WORK/gate2.log"
fi
git -C "$ROOT" archive HEAD -- "$ROOM" | tar -x -C "$WORK"
stage_inputs || exit 2

# --- injection 1: the defect the identity gate exists for ---------------------
echo "=== injection 1 — the channel writes into the world ==="
inject_1
if ! (cd "$WORK" && cargo build --release -p worldgen-dump -p region-differ \
        > "$WORK/build1.log" 2>&1); then
    echo "REFUSED  the injected tree did not build — the injection is wrong,"
    echo "         not the guard."
    tail -20 "$WORK/build1.log"
    rc=1
elif (cd "$WORK" && ./ci/be-channel-identical.sh > "$WORK/gate1.log" 2>&1); then
    echo "REFUSED  ci/be-channel-identical.sh reported GREEN while the channel"
    echo "         was writing blocks. The gate cannot fail."
    tail -5 "$WORK/gate1.log"
    rc=1
else
    # WHICH refusal, not just that there was one. A gate that refused as
    # VACUOUS did not catch this with the comparison, and reporting either as
    # "caught" would credit the wrong instrument.
    if grep -q 'recording changed generation' "$WORK/gate1.log"; then
        echo "  caught by the comparison: $(grep -m1 -E 'mismatches between' "$WORK/gate1.log")"
        echo "                            $(grep -m1 'recording changed generation' "$WORK/gate1.log")"
    else
        echo "REFUSED  ci/be-channel-identical.sh went red on injection 1, but NOT"
        echo "         with 'recording changed generation' — so the block-state"
        echo "         comparison is not what caught it. What did:"
        grep -m2 -E 'VACUOUS|REFUSED|FAIL' "$WORK/gate1.log" | sed 's/^/         /'
        rc=1
    fi
fi

# --- and the clean tree must still pass, or a red proves nothing --------------
echo "=== control — the unpatched tree ==="
git -C "$ROOT" archive HEAD | tar -x -C "$WORK"
stage_inputs || exit 2
if (cd "$WORK" && cargo build --release -p worldgen-dump -p region-differ \
        > "$WORK/build0.log" 2>&1) && (cd "$WORK" && ./ci/be-channel-identical.sh \
        > "$WORK/gate0.log" 2>&1); then
    echo "  the clean tree is green, so the reds above are the injections"
else
    echo "REFUSED  the CLEAN tree does not pass its own gate — every red above"
    echo "         is unattributable."
    tail -5 "$WORK/gate0.log"
    rc=1
fi

if [ "$rc" -eq 0 ]; then
    echo "GREEN  both defects were caught by the instrument that owns them, and"
    echo "       the clean tree passes."
else
    echo "FAIL   $WORK kept for inspection"
    exit "$rc"
fi
rm -rf "$WORKROOT"
