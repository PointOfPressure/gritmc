#!/usr/bin/env bash
# reference-cache-integration.sh — prove the cache actually changes what
# `ci/t2-gameplay.sh` DOES, not just what `refcache_lookup` returns.
#
# `ci/reference-cache-falsify.sh` tests the cache in isolation: twenty cases on
# a synthetic tree, every mutated input missing and naming itself. That is the
# safety argument and it is necessary. It is not sufficient, because the whole
# claim of this branch is a claim about the *harness*:
#
#     on a hit, no vanilla boots, and 21.3 minutes of exclusive lock are not spent
#
# A cache that returned perfect answers into a script that booted vanilla anyway
# would pass every case in the falsifier and save nothing — and would do it
# silently, because the recording would be correct and the gate would be green.
# That is a saving whose absence is invisible, which is the same shape as a
# check whose green cannot go red.
#
# So this runs THE REAL `ci/t2-gameplay.sh`, twice, in a scratch tree, with
# `ci/vanilla-server.sh` replaced by a stub that records every invocation and
# refuses to start anything:
#
#   miss  cache empty  -> the stub's `start` MUST be reached
#   hit   cache seeded -> the stub's `start` MUST NOT be reached, and the run
#                         must get as far as booting the subject
#
# Both runs end in a failure, because nothing here can boot a real server. What
# is asserted is WHICH failure and what happened on the way to it — the marker
# file is the instrument, and it is a file the harness cannot write by accident.
#
# Boots nothing, needs no jar and no JVM, and takes its corpus lock in a private
# namespace via `RUSTMC_CORPUS_LOCK_DIR`, so it cannot touch real work. About
# five seconds. `ci/handoff-verdict-selftest.sh` is the pattern — a real script
# driven end to end against stubs rather than a reimplementation of it.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/refcache-integ.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
T="$WORK/tree"
export REFCACHE_DIR="$WORK/cache"
export RUSTMC_CORPUS_LOCK_DIR="$WORK/lock"
export CORPUS_LOCK_ANCHOR_NAME="refcache-integ-anchor-$$"
export CORPUS_LOCK_HEARTBEAT=0
mkdir -p "$RUSTMC_CORPUS_LOCK_DIR"

PASS=0; FAIL=0
ok()  { PASS=$((PASS+1)); printf '  ok    %s\n' "$*"; }
bad() { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# --- the scratch tree --------------------------------------------------------
build_tree() {
    rm -rf "$T"; mkdir -p "$T/target/release" "$T/data/vanilla" "$T/crates/fake/src" \
        "$T/crates/rustmc-proto/src" \
        "$T/tools/oracle-runner/src" "$T/oracle-work/gameplay" \
        "$T/oracle-work/vanilla-26.2/versions/26.2" "$T/bin"
    cp -r "$ROOT/ci" "$T/ci"
    cp "$ROOT"/tools/oracle-runner/src/*.rs "$T/tools/oracle-runner/src/"
    printf 'not a real jar\n' > "$T/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"

    # THE SAME TWO FILES `ci/reference-cache-falsify.sh` MAKES, AND FOR THE SAME
    # REASON. `a55be66bc` added `proto=` to the cache key — the recorder's
    # decoder is written against `rustmc_proto`, so a decoder change used to
    # move no component of it — and updated the FALSIFIER's synthetic tree with
    # exactly this pair, saying why: without them "the harness computes
    # `proto=MISSING` and every lookup refuses, so the suite passes while
    # testing nothing". **This script has a synthetic tree too and it was not
    # updated**, so from that commit until 2026-09-05 every `hit` case here
    # missed on `proto=MISSING`, booted the stub, and the suite reported
    # `2 passed, 4 failed`. It failed loudly rather than quietly, which is the
    # lucky direction and not a reason to leave the second tree behind the
    # first: a component added to `refcache_components` has to be added to
    # BOTH, and the next one will too.
    printf 'pub fn read_varint() {}\n' > "$T/crates/rustmc-proto/src/varint.rs"
    printf 'pub fn decode() {}\n'      > "$T/crates/rustmc-proto/src/wire.rs"

    # The preflight derives its REQUIRED table list by grepping crates/, tools/
    # and plugins/ for `"data/vanilla/<name>.json"`. Naming exactly the four the
    # harness opens keeps the scratch preflight honest and small: these really
    # are the tables `client::Tables::load` reads.
    #
    # **`protocol_ids.json` is the fourth, and leaving it out did not fail
    # loudly.** `corpus-sound-pitch` added it to `Tables::load` and to the
    # reference-cache key; this tree did not have it, so `refcache_key`
    # produced `tables=MISSING`, the cache could never hit, and the `hit` cases
    # went on to boot the stub reference server and fail there — reported as
    # `vanilla died before it finished booting`, which reads like a server
    # problem and is a missing fixture file.
    cat > "$T/crates/fake/src/lib.rs" <<'EOS'
const A: &str = "data/vanilla/items.json";
const B: &str = "data/vanilla/block_props.json";
const C: &str = "data/vanilla/entity_types.json";
const D: &str = "data/vanilla/protocol_ids.json";
EOS
    printf '{"items":{"minecraft:stone":1}}\n'         > "$T/data/vanilla/items.json"
    printf '{"table":[{"id":1,"state":"stone"}]}\n'    > "$T/data/vanilla/block_props.json"
    printf '{"entity_types":{"minecraft:player":0}}\n' > "$T/data/vanilla/entity_types.json"
    printf '{"sound_event":{"minecraft:block.lever.click":0}}\n' \
                                                      > "$T/data/vanilla/protocol_ids.json"
    ( . "$T/ci/provenance.sh"
      for t in items block_props entity_types protocol_ids; do
          prov_stamp "$T" "$T/data/vanilla/$t.json" ci/gen-gameplay-tables.sh
      done )

    # The reference server, stubbed. `start` leaves a marker and refuses; every
    # other verb is a no-op. The marker is the whole instrument of this test.
    cat > "$T/ci/vanilla-server.sh" <<EOS
#!/usr/bin/env bash
echo "\$1" >> "$WORK/vanilla-server.calls"
case "\$1" in
    start) echo "STUB: vanilla-server start was reached" >&2; exit 1 ;;
    alive) exit 0 ;;
    *) exit 0 ;;
esac
EOS
    chmod +x "$T/ci/vanilla-server.sh"

    # The runner, stubbed. Only `projection-id` is reached in either run — the
    # cache asks for it, and neither run gets as far as recording or comparing.
    cat > "$T/target/release/oracle-runner" <<'EOS'
#!/usr/bin/env bash
[ "${1:-}" = "projection-id" ] && { echo "proj:f7fbb29926074663"; exit 0; }
echo "STUB oracle-runner: $*" >&2; exit 1
EOS
    chmod +x "$T/target/release/oracle-runner"
    printf '#!/usr/bin/env bash\nexit 0\n' > "$T/bin/cargo"; chmod +x "$T/bin/cargo"
    # `rustmc-server` must exist and must exit, so the hit run stops at the
    # subject boot — a legible, expected end rather than a hang.
    printf '#!/usr/bin/env bash\nexit 0\n' > "$T/target/release/rustmc-server"
    chmod +x "$T/target/release/rustmc-server"
    rm -f "$WORK/vanilla-server.calls"
}

# A recording covering this corpus, so `assert_recording` passes on the served
# file. Its scenario COUNT is what that assertion reads, and getting it wrong is
# the failure that assertion exists for — so it is derived from the corpus, not
# written as a number.
seed_recording() {
    local n; n=$(find "$T/ci/checks/gameplay" -maxdepth 1 -name '*.toml' | wc -l)
    python3 -c "
import json,sys
n=int(sys.argv[1])
json.dump({'target':'127.0.0.1:0','version':'26.2','protocol':776,
           'recorder':'oracle-runner 0.1.0',
           'scenarios':[{'name':f's{i}','anchor':[0,0,0],'spawn':[0,0,0],
                         'obs':[],'fresh_session':False} for i in range(n)]},
          open(sys.argv[2],'w'))" "$n" "$WORK/seed.json"
}

# `RUSTMC_CORPUS_SHARDS=1`: this asks what the CACHE does to a run, and its
# stub vanilla answers one console on one port pair. Since `t2-gameplay.sh`
# defaults to 8 shards (2026-10-01) an unpinned run boots eight stubs, all
# eight refuse, and every case here reds for a reason that is not the cache.
# The shard key itself is `ci/reference-cache-falsify.sh`'s.
run_gate() {
    ( cd "$T" && PATH="$T/bin:$PATH" RUSTMC_CORPUS_SHARDS=1 \
        RUSTMC_REFERENCE_CACHE=1 REFCACHE_DIR="$REFCACHE_DIR" \
        RUSTMC_CORPUS_LOCK_DIR="$RUSTMC_CORPUS_LOCK_DIR" \
        CORPUS_LOCK_HEARTBEAT=0 \
        timeout 180 ./ci/t2-gameplay.sh ) > "$1" 2>&1
    echo $?
}

echo "== reference-cache-integration: scratch tree, stubbed servers, private lock"
echo

# --- run 1: cache empty, the reference must be recorded ----------------------
build_tree
RC="$(run_gate "$WORK/miss.log")"
if grep -q 'vanilla-server start was reached' "$WORK/miss.log" \
   && grep -qx 'start' "$WORK/vanilla-server.calls" 2>/dev/null; then
    ok "miss — an empty cache reaches \`vanilla-server start\` (the reference is recorded)"
else
    bad "miss — never reached vanilla-server start (rc=$RC): $(tail -3 "$WORK/miss.log" | tr '\n' ' ')"
fi
grep -q 'reference-cache: MISS' "$WORK/miss.log" \
    && ok "miss — the run says MISS and why" \
    || bad "miss — no MISS line in the log"

# --- run 2: cache seeded, the reference must be served -----------------------
# Same tree, so every key component is identical; only the cache is different.
# That is the control: nothing about the tree changed between the two runs, so a
# difference in behaviour can only be the cache.
build_tree
seed_recording
( cd "$T" && . "$T/ci/reference-cache.sh" \
  && RUSTMC_CORPUS_SHARDS=1 RUSTMC_REFERENCE_CACHE=1 refcache_store "$T" "$T/target/release/oracle-runner" \
        20260803 "$WORK/seed.json" ) >/dev/null 2>&1
RC="$(run_gate "$WORK/hit.log")"

if grep -q 'reference-cache: HIT' "$WORK/hit.log"; then
    ok "hit — the seeded reference is served"
else
    bad "hit — no HIT line (rc=$RC): $(tail -4 "$WORK/hit.log" | tr '\n' ' ')"
fi

# THE claim of this branch.
if [ -s "$WORK/vanilla-server.calls" ] && grep -qx 'start' "$WORK/vanilla-server.calls"; then
    bad "no-boot — vanilla was STARTED on a cache hit; the hold is not reduced"
else
    ok "no-boot — vanilla-server \`start\` is never invoked on a hit"
fi

# ...and the run must not merely skip the reference, it must carry on with it.
# A hit that also skipped the subject would "save" the whole gate by measuring
# nothing, which is the failure this assertion exists to separate from a saving.
if grep -q 'booting rustmc' "$WORK/hit.log"; then
    ok "hit — the run continues to the subject half with the served reference"
else
    bad "hit — never reached the subject boot: $(tail -4 "$WORK/hit.log" | tr '\n' ' ')"
fi

# The served recording must have passed the corpus's own denominator check
# rather than bypassing it.
if grep -q 'vanilla (cached): .*scenarios recorded' "$WORK/hit.log"; then
    ok "hit — the served recording still goes through assert_recording"
else
    bad "hit — assert_recording did not run on the served recording"
fi

echo
echo "reference-cache-integration: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || { echo; echo "--- miss.log tail ---"; tail -15 "$WORK/miss.log"
                       echo "--- hit.log tail ---";  tail -15 "$WORK/hit.log"; exit 1; }
