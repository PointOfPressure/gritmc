#!/usr/bin/env bash
# ===== AGENT INTERFACE — this is all you need to read =====
#
#     ci/chunk-send-unrecorded-falsify.sh
#
# Proves `ci/chunk-send-unrecorded.py` can go red. Seven cases: one positive
# control, one reconstruction of the real shipped defect, and five injections
# covering each refusal verb. Runs in a scratch copy of the tree, touches
# nothing, takes about a second.
# ===== end interface; everything below is why =====
#
# A guard that cannot be shown refusing a real past instance is itself a check
# that cannot fail — `docs/AGENT-BRIEF.md`'s first hard rule, and the reason
# `ci/unfalsifiable-falsify.py` exists. Case 2 below is not invented: on
# `origin/main` at f82b2ebbf, `trace::is_housekeeping` contained
# `LEVEL_CHUNK_WITH_LIGHT`, and `docs/PARITY-ROADMAP.md` §31 and §32 were both
# written against that projection — a block-entity channel built into a packet
# no recording contained. Case 2 restores exactly that line and requires the
# guard to refuse it.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
# Copy only the four paths the guard reads. Copying `crates` whole would put a
# 40 MB tree behind a one-second check, and would make this harness sensitive
# to files it does not test.
mkdir -p "$WORK/ci/checks" "$WORK/tools/oracle-runner/src" "$WORK/crates/rustmc-proto/src"
cp "$ROOT/ci/chunk-send-unrecorded.py" "$WORK/ci/"
cp "$ROOT/ci/checks/chunk-send-unrecorded.toml" "$WORK/ci/checks/"
cp "$ROOT/tools/oracle-runner/src/trace.rs" "$WORK/tools/oracle-runner/src/"
cp "$ROOT/crates/rustmc-proto/src/ids.rs" "$WORK/crates/rustmc-proto/src/"
TRACE="$WORK/tools/oracle-runner/src/trace.rs"
LEDGER="$WORK/ci/checks/chunk-send-unrecorded.toml"
GUARD="$WORK/ci/chunk-send-unrecorded.py"

pass=0; fail=0
run() { (cd "$WORK" && python3 "$GUARD" 2>&1); }
restore() { cp "$ROOT/tools/oracle-runner/src/trace.rs" "$TRACE"
            cp "$ROOT/ci/checks/chunk-send-unrecorded.toml" "$LEDGER"; }

expect_refusal() {   # <case> <verb>
    local name="$1" verb="$2" out rc
    out=$(run); rc=$?
    if [ $rc -eq 0 ]; then
        echo "FAIL  $name — guard ACCEPTED an injection it must refuse"; fail=$((fail+1))
    elif ! grep -q "$verb" <<<"$out"; then
        echo "FAIL  $name — refused, but not with $verb:"; sed 's/^/        /' <<<"$out"; fail=$((fail+1))
    else
        echo "ok    $name — refused with $verb"; pass=$((pass+1))
    fi
    restore
}

# 1. Positive control. An untouched tree must be accepted, or every refusal
#    below proves only that the guard refuses everything.
out=$(run); rc=$?
if [ $rc -eq 0 ]; then echo "ok    control — the real tree is accepted"; pass=$((pass+1))
else echo "FAIL  control — the real tree is REFUSED:"; sed 's/^/        /' <<<"$out"; fail=$((fail+1)); fi

# 2. THE REAL DEFECT. Put LEVEL_CHUNK_WITH_LIGHT back into the drop list,
#    exactly as origin/main f82b2ebbf carries it.
# Every injection below is written in the recorder's CURRENT shape: an early
# `return None` in `trace::observe` under a test naming the kind. Until
# 2026-09-24 they edited `is_housekeeping`'s `matches!` arm, which
# `recorder-housekeeping` deleted.
drop_in_observe() {   # <cb name> — make observe drop that kind at record time
    python3 - "$TRACE" "$1" <<'PY'
import sys
p, name = sys.argv[1], sys.argv[2]; s = open(p).read()
a = ") -> Option<(String, BTreeMap<String, serde_json::Value>)> {\n    if let Some((kind, mut f, consumed)) = decode("
assert s.count(a) == 1, "anchor moved — this case has stopped reconstructing anything"
head, rest = a.split("\n", 1)
open(p, "w").write(s.replace(a, head + f"\n    if id == ids::play::cb::{name} {{\n        return None;\n    }}\n" + rest, 1))
PY
}
add_row() {   # <cb name> <id> <impact>
    cat >> "$LEDGER" <<ROW

[[kind]]
name = "$1"
id = $2
recorded = false
sent_by = "injected by the falsifier"
wire = "injected"
impact = "$3"
why = "injected by ci/chunk-send-unrecorded-falsify.sh"
residual = "injected"
ROW
}

# 1. The shipped defect: LEVEL_CHUNK_WITH_LIGHT dropped at record time again,
#    with no row. This is what §31 and §32 were written against.
drop_in_observe LEVEL_CHUNK_WITH_LIGHT
expect_refusal "the shipped defect (LEVEL_CHUNK_WITH_LIGHT re-dropped)" UNLEDGERED

# 2. A row outliving its drop: the ledger says FORGET_LEVEL_CHUNK is unrecorded
#    and observe records it.
add_row FORGET_LEVEL_CHUNK 0x25 armed
expect_refusal "a row outliving its drop" STALE-ROW

# 3. A drop whose row was deleted (or never written): LIGHT_UPDATE.
drop_in_observe LIGHT_UPDATE
expect_refusal "a deleted row" UNLEDGERED

# 4. A row whose id drifted from ids.rs, on a kind that IS dropped.
drop_in_observe FORGET_LEVEL_CHUNK
add_row FORGET_LEVEL_CHUNK 0x26 armed
expect_refusal "an id drifting from ids.rs" ID-DRIFT

# 5. An impact outside the vocabulary, on a kind that IS dropped.
drop_in_observe FORGET_LEVEL_CHUNK
add_row FORGET_LEVEL_CHUNK 0x25 "probably fine"
expect_refusal "an impact outside the vocabulary" BAD-IMPACT

# 6. The function the guard reads, renamed away: it must refuse, not pass on
#    an empty drop set.
python3 - "$TRACE" <<'PY'
import sys
p = sys.argv[1]; s = open(p).read()
out = s.replace("pub fn observe(", "pub fn observe_packet(", 1)
assert out != s
open(p, "w").write(out)
PY
expect_refusal "observe rewritten away" "could not find"

echo
echo "chunk-send-unrecorded-falsify: $pass passed, $fail failed"
[ $fail -eq 0 ]
