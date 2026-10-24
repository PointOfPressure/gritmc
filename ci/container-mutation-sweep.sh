#!/usr/bin/env bash
# container-mutation-sweep.sh — prove the container tests can fail.
#
#     ./ci/container-mutation-sweep.sh
#     ./ci/container-mutation-sweep.sh /path/to/logfile
#
# # WHY THIS EXISTS
#
# ORACLE rule 27: `AbstractContainerMenu.broadcastChanges` answers **only**
# where the server's truth differs from the client's prediction, so a correct
# server answers a correct prediction with silence — and a server with no
# containers at all also answers with silence. The two are indistinguishable
# on the wire. A container test that merely observes "no divergence" is
# therefore compatible with an implementation that does nothing at all.
#
# Defect class #19 is the sharper version and is what this script is really
# for. Eight rail tests once passed under four separately injected bugs,
# because the scenario those tests drove never reached a whole branch of the
# method under test. Pairing a test with its own scenario is not evidence.
# **Tests must target the method's branches.**
#
# So: inject a specific, plausible bug into a specific branch, and assert that
# a specific named test goes red. A mutation that nothing catches is a branch
# with no coverage, and it is reported as a FAILURE of this sweep rather than
# as a passing run.
#
# # HOW TO READ THE OUTPUT
#
#   caught   <mutation>  by <test>     the branch has a test that pins it
#   MISSED   <mutation>                nothing failed: an untested branch
#
# A mutation that fails to *compile* is also a MISS — it means the sweep is
# stale with respect to the code and is silently measuring nothing, which is
# the exact failure mode it exists to catch.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOST="${RUSTMC_REMOTE:-threadripper}"
. "$(dirname "$0")/lib/require-remote-dir.sh"
DEST="$RUSTMC_REMOTE_DIR"
LOG="${1:-$ROOT/../logs/container-mutation-$(date +%Y%m%d-%H%M%S).log}"
mkdir -p "$(dirname "$LOG")"

CLICK="$ROOT/crates/rustmc-server/src/click.rs"
BE="$ROOT/crates/rustmc-server/src/blockentity.rs"
ITEM="$ROOT/crates/rustmc-server/src/item.rs"
RECIPES="$ROOT/crates/rustmc-server/src/recipes.rs"

# Every mutation is `file<TAB>from<TAB>to<TAB>description`. `from` must occur
# exactly once in the file — the script checks, because a `from` that matches
# nothing would silently test the unmutated code and report a MISS that looks
# like missing coverage rather than a stale script.
MUTATIONS=$(cat <<'MUT'
click	if known && believed == actual {	if false {	broadcast_changes answers EVERY slot (the over-eager server 561 forbids)
click	if known && believed == actual {	if true {	broadcast_changes answers NOTHING (the do-nothing server 560 forbids)
click	out.push(Reply::SetCursor {	out.push(Reply::SetSlot { container_id: 0, state_id: 0, slot: -1, stack: ItemStack::empty() }); let _ = (Reply::SetCursor {	the cursor answered as container_set_slot, which also bumps the state id
click	Some(SlotRef::Result) => false,	Some(SlotRef::Result) => true,	a crafting result slot accepts a placement
click	Some(SlotRef::Container(_)) => self.container.is_some(),	Some(SlotRef::Container(_)) => true,	a vanished block entity accepts a placement and destroys the stack
click	Some(SlotRef::Inventory(MAIN_INVENTORY_START + j))	Some(SlotRef::Inventory(j))	the menu-to-inventory slot map shifts instead of reversing
click	.min(self.max_stack_size(i, &stack) - existing.count());	;	safeInsert ignores the slot maximum
click	ClickAction::Secondary => (slot_stack.count() + 1) / 2,	ClickAction::Secondary => slot_stack.count() / 2,	a right-click takes the smaller half
click	let mismatch = state_id != slots.menu.state_id;	let mismatch = false;	a stale state id is answered with a correction instead of the whole menu
click	if !slots.menu.is_valid_slot_index(slot_num) {	if false {	an out-of-range slot index is not refused
click	if container_id != slots.menu.container_id {	if false {	a click naming another window is acted on
be	(Some(had), Some(now)) if had == now => false,	(Some(had), Some(now)) if had != now => false,	a chest loses its contents when its own state changes
be	                map.remove(&pos);	                let _ = &pos;	a broken block keeps its block entity
be	        c.insert("Items".to_string(), Nbt::List(items));	        if !items.is_empty() { c.insert("Items".to_string(), Nbt::List(items)); }	the Items key is dropped when the container is empty
item	if count <= 0 || item.is_empty() || item == "minecraft:air" {	if false {	a zero-count stack is not empty
item	let taken = amount.min(self.count);	let taken = amount;	split can take more than the stack holds
click	self.grid_written = true;	let _: () = ();	a grid write does not trigger the recipe
click	if let Some(b) = slots.menu.remote_slots.get_mut(0) {	if let Some(b) = slots.menu.remote_slots.get_mut(usize::MAX) {	slotChangedCraftingGrid does not update the belief, so slot 0 is sent twice
click	if slots.menu.spec.kind != MenuKind::Crafting {	if false {	a chest click runs the crafting recipe
recipes	if input.ingredient_count() != ingredients.len() {	if false {	a shapeless recipe matches with stacks left over
recipes	if !stack.is_empty() {	if false {	a hole in a shaped pattern accepts any item
recipes	if input.width != *width || input.height != *height {	if false {	a shaped recipe matches a grid of the wrong size
MUT
)

echo "container-mutation-sweep: $(echo "$MUTATIONS" | grep -c .) mutations" | tee "$LOG"

run_tests() {
    rsync -az --delete --exclude target --exclude oracle-work --exclude .git \
        --exclude 'data/vanilla' "$ROOT/" "$HOST:$DEST/" >/dev/null 2>&1 || return 99
    ssh -n "$HOST" "export PATH=\$HOME/.cargo/bin:\$PATH; cd $DEST && \
        cargo test --release -p rustmc-server --lib 2>&1" 2>&1
}

echo "--- control: the unmutated tree must be GREEN ---" | tee -a "$LOG"
CONTROL="$(run_tests)"
if ! grep -q "test result: ok" <<<"$CONTROL"; then
    echo "control run is not green — fix that before believing any mutation" | tee -a "$LOG"
    grep -E "^error|FAILED|failures:" <<<"$CONTROL" | head -20 | tee -a "$LOG"
    exit 2
fi
echo "control GREEN: $(grep -oE 'test result: ok\. [0-9]+ passed' <<<"$CONTROL" | head -1)" | tee -a "$LOG"

CAUGHT=0
MISSED=0
while IFS=$'\t' read -r which from to desc; do
    [ -z "${which:-}" ] && continue
    case "$which" in
        click) FILE="$CLICK" ;;
        be)    FILE="$BE" ;;
        item)  FILE="$ITEM" ;;
        recipes) FILE="$RECIPES" ;;
        *)     echo "unknown file key $which" | tee -a "$LOG"; exit 2 ;;
    esac
    n=$(grep -cF -- "$from" "$FILE")
    if [ "$n" -ne 1 ]; then
        echo "MISSED  $desc" | tee -a "$LOG"
        echo "        (the pattern matched $n times, not 1 — this script is stale)" | tee -a "$LOG"
        MISSED=$((MISSED + 1))
        continue
    fi
    cp -p "$FILE" "$FILE.orig"
    python3 - "$FILE" "$from" "$to" <<'PY'
import sys
path, frm, to = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(path).read()
open(path, "w").write(s.replace(frm, to, 1))
PY
    # **`touch` is not cosmetic.** `cp` + `mv` gives the restored file the
    # mtime it had when the copy was taken, which is EARLIER than the mutated
    # version rsync already pushed. Cargo fingerprints on mtime, decides the
    # source is older than the build output, and reuses the binary it built
    # from the MUTATED source. The first version of this script did exactly
    # that: every run after the first tested the previous mutation's binary,
    # and the control then failed with `split` returning 99 where it should
    # return 2 — a mutation restored in the source and alive in the artefact.
    touch "$FILE"
    OUT="$(run_tests)"
    mv "$FILE.orig" "$FILE"
    touch "$FILE"

    # **`^error:` is NOT a compile-error test**, and reading it as one made
    # the first run of this script report 0 caught and 16 missed. `cargo test`
    # prints `error: test failed, to rerun pass ...` when a test *fails* —
    # which is the success case here — so a mutation that worked perfectly was
    # being filed as one that did not build. Rule 7: when a measurement clears
    # every candidate, suspect the measurement. Match what rustc actually
    # emits for a build failure instead.
    if grep -qE "^error\[E[0-9]+\]|^error: could not compile|^error: expected|^error: cannot" <<<"$OUT"; then
        echo "MISSED  $desc" | tee -a "$LOG"
        echo "        (the mutation did not compile — it tested nothing)" | tee -a "$LOG"
        MISSED=$((MISSED + 1))
        continue
    fi
    # **Not anchored at the start of the line**, and the first version was.
    # `cargo test` writes `test click::tests::name ... FAILED`, so `^` never
    # matched and every mutation was filed as uncaught — the second way this
    # script measured itself rather than the code. The `test result: FAILED`
    # line is the authority on whether anything failed at all; the names are
    # scraped for the report.
    if grep -q "test result: FAILED" <<<"$OUT"; then
        FAILED=$(grep -oE '[a-z_]+::tests::[a-z_0-9]+' <<<"$OUT" | sort -u | tr '\n' ' ')
        echo "caught  $desc" | tee -a "$LOG"
        echo "        by: ${FAILED:-(named tests not scraped)}" | tee -a "$LOG"
        CAUGHT=$((CAUGHT + 1))
    else
        echo "MISSED  $desc" | tee -a "$LOG"
        echo "        NOTHING FAILED — this branch has no test that pins it" | tee -a "$LOG"
        MISSED=$((MISSED + 1))
    fi
done <<<"$MUTATIONS"

# Restore, whatever happened, and prove it.
ssh -n "$HOST" "true" >/dev/null 2>&1
echo "container-mutation-sweep: $CAUGHT caught, $MISSED missed" | tee -a "$LOG"
echo "log: $LOG"
[ "$MISSED" -eq 0 ]
