#!/usr/bin/env bash
# reference-cache-falsify.sh — prove `ci/reference-cache.sh` goes red when it
# should.
#
# A cache that never misses is not a cache, it is a stale reference with a
# hit-rate counter. This project's central defect is a check whose green cannot
# be turned red (`docs/AGENT-BRIEF.md`), and a reference cache is the most
# dangerous possible instance of it: a stale serve does not break a gate, it
# makes every gate pass against an artefact nobody re-derived. So the machinery
# ships with the evidence that it refuses.
#
# Every case mutates ONE key input of a synthetic tree, and asserts:
#
#   * the lookup MISSES, and
#   * the miss names the component that moved.
#
# The second half matters as much as the first. A cache that misses for the
# wrong reason is one edit away from missing for no reason, and "it went red"
# is not evidence that it went red *because of the thing you changed* — the
# same distinction `ci/corpus-falsifiable.py` draws between tier 1 and tier 2.
#
# Runs entirely on a synthetic tree in a scratch directory with its own
# REFCACHE_DIR, so it touches no real entry, needs no vanilla jar, boots
# nothing, and **takes no lock**. About two seconds.
#
#     ./ci/reference-cache-falsify.sh
#
# Patterned on `ci/corpus-lock-falsify.sh` (private namespace, injection per
# case) and `ci/lanes-falsify.sh`.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/refcache-falsify.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
export REFCACHE_DIR="$WORK/cache"
export RUSTMC_REFERENCE_CACHE=1
unset RUSTMC_CORPUS_SHARDS

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); printf '  ok    %s\n' "$*"; }
bad()  { FAIL=$((FAIL+1)); printf '  FAIL  %s\n' "$*"; }

# --- the synthetic tree ------------------------------------------------------
# Every path `refcache_components` reads, and nothing else. Built fresh per
# case, so a case cannot leak into the next one — the failure mode that makes a
# sweep of nineteen injections report nineteen passes for one working patch.
mktree() {
    local t="$1"
    rm -rf "$t"; mkdir -p "$t/ci/checks/gameplay" "$t/tools/oracle-runner/src" \
        "$t/oracle-work/vanilla-26.2/versions/26.2" "$t/data/vanilla"
    printf 'not a real jar, but a fixed byte sequence\n' > "$t/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
    printf 'name="break-block"\n[[step]]\nkind="dig"\n'  > "$t/ci/checks/gameplay/001-break-block.toml"
    printf 'name="place-block"\n[[step]]\nkind="place"\n' > "$t/ci/checks/gameplay/002-place-block.toml"
    printf 'pub fn record() {}\n'  > "$t/tools/oracle-runner/src/record.rs"
    printf 'pub fn compare() {}\n' > "$t/tools/oracle-runner/src/compare.rs"
    # The recorder's decoder is `trace.rs`, which is written against
    # `rustmc_proto`; the key covers that crate for the reason its own comment
    # in `refcache_components` gives. Without these two files every tree this
    # harness builds computes `proto=MISSING` and every lookup refuses, so the
    # suite would pass while testing nothing — the exact shape this repo calls
    # an unfalsifiable check.
    mkdir -p "$t/crates/rustmc-proto/src"
    printf 'pub fn read_varint() {}\n' > "$t/crates/rustmc-proto/src/varint.rs"
    printf 'pub fn decode() {}\n'      > "$t/crates/rustmc-proto/src/wire.rs"
    cp "$ROOT/ci/t2-gameplay.sh"     "$t/ci/t2-gameplay.sh"
    cp "$ROOT/ci/vanilla-server.sh"  "$t/ci/vanilla-server.sh"
    cp "$ROOT/ci/reference-cache.sh" "$t/ci/reference-cache.sh"
    # The substrate the reference is recorded UNDER, added to the key
    # 2026-09-05: the four game rules and the difficulty vanilla boots at, both
    # moved out of `ci/t2-gameplay.sh` into files of their own. A fixture
    # missing them makes the whole `harness` component MISSING, which is this
    # suite noticing the key grew rather than a defect.
    cp "$ROOT/ci/gameplay-substrate.sh"  "$t/ci/gameplay-substrate.sh"
    cp "$ROOT/ci/gameplay-difficulty.sh" "$t/ci/gameplay-difficulty.sh"
    printf '{"items":{"minecraft:stone":1}}\n'        > "$t/data/vanilla/items.json"
    printf '{"table":[{"id":1,"state":"stone"}]}\n'   > "$t/data/vanilla/block_props.json"
    printf '{"entity_types":{"minecraft:player":0}}\n'> "$t/data/vanilla/entity_types.json"
    # The fourth table the key covers. Without it every case above reads
    # `tables=MISSING` and 12 of them fail for a reason that has nothing to do
    # with what they test.
    printf '{"sound_event":{"minecraft:block.lever.click":0}}\n' > "$t/data/vanilla/protocol_ids.json"
    # A stand-in for the binary. The real one answers `projection-id` with
    # `proj:<digest>`; this answers with whatever $PROJ says, which is how the
    # projection case moves that component and nothing else.
    mkdir -p "$t/bin"
    cat > "$t/bin/oracle-runner" <<'EOS'
#!/usr/bin/env bash
[ "${1:-}" = "projection-id" ] && { echo "${PROJ:-proj:f7fbb29926074663}"; exit 0; }
exit 2
EOS
    chmod +x "$t/bin/oracle-runner"
    printf '{"target":"127.0.0.1:0","scenarios":[{"name":"break-block"},{"name":"place-block"}]}\n' \
        > "$t/recording.json"
}

TREE="$WORK/tree"
BIN() { echo "$TREE/bin/oracle-runner"; }
SEED=20260803

# `. reference-cache.sh` from the REAL tree — the subject under test is the
# file that will ship, not the copy inside the synthetic tree (which exists
# only to be hashed as a harness input).
. "$ROOT/ci/reference-cache.sh"

# Store a baseline entry, then run a mutation and report what the lookup did.
# Returns the lookup's stderr+stdout so a case can assert on the REASON.
seed_entry() {
    mktree "$TREE"
    refcache_store "$TREE" "$(BIN)" "$SEED" "$TREE/recording.json" >/dev/null 2>&1
}
try_lookup() {
    refcache_lookup "$TREE" "$(BIN)" "$SEED" "$WORK/out.json" 2>&1
}

echo "== reference-cache-falsify: $(basename "$REFCACHE_DIR") is a private namespace"
echo

# --- case 1: the positive control -------------------------------------------
# Without this every case below is satisfied by a cache that never hits at all.
seed_entry
OUT="$(try_lookup)"; RC=$?
if [ $RC -eq 0 ] && printf '%s' "$OUT" | grep -c 'HIT' >/dev/null; then
    if cmp -s "$WORK/out.json" "$TREE/recording.json"; then
        ok "hit — unchanged inputs serve the stored recording, byte-identical"
    else bad "hit — served a file that is not the stored recording"; fi
else bad "hit — unchanged inputs did NOT hit (rc=$RC): $OUT"; fi

# --- case 2: content-addressed, not mtime-addressed --------------------------
# `rsync -a` preserves mtimes and cargo trusts them; that has already cost this
# project a measurement (docs/AGENT-BRIEF.md). A key that moved when a file was
# merely touched would miss on every rsync and the cache would be worthless; a
# key that ignored content would be worse. This asserts the first half.
K1="$(refcache_key "$TREE" "$(BIN)" "$SEED")"
find "$TREE" -type f -exec touch {} +
K2="$(refcache_key "$TREE" "$(BIN)" "$SEED")"
[ "$K1" = "$K2" ] && ok "mtime — touching every file leaves the key unmoved (content-addressed)" \
                  || bad "mtime — the key moved when only mtimes did: $K1 -> $K2"
OUT="$(try_lookup)"
printf '%s' "$OUT" | grep -c 'HIT' >/dev/null && ok "mtime — still a hit after a full touch" \
                                   || bad "mtime — a touch turned a hit into a miss"

# --- cases 3..10: one mutated component each ---------------------------------
# `component` is the name that MUST appear in the miss. Asserting the name is
# what makes this tier 2 rather than tier 1: a miss that does not name the
# thing you moved is a cache that missed for some other reason.
mutate_case() {
    local name="$1" component="$2"; shift 2
    seed_entry
    "$@"                      # the mutation, applied after the entry is stored
    local out rc
    out="$(try_lookup)"; rc=$?
    if [ $rc -eq 0 ]; then
        bad "$name — SERVED A STALE REFERENCE after $component moved"
        return
    fi
    if printf '%s' "$out" | grep -c "$component" >/dev/null; then
        ok "$name — miss, and the miss names \`$component\`"
    else
        bad "$name — missed but did not name \`$component\`: $(printf '%s' "$out" | head -2 | tr '\n' ' ')"
    fi
}

mutate_case "scenario-content" scenarios \
    bash -c 'printf "\n[[step]]\nkind=\"use\"\n" >> "$0/ci/checks/gameplay/001-break-block.toml"' "$TREE"
mutate_case "scenario-rename" scenarios \
    bash -c 'mv "$0/ci/checks/gameplay/002-place-block.toml" "$0/ci/checks/gameplay/002-place-blocks.toml"' "$TREE"
mutate_case "scenario-added" n_checks \
    bash -c 'printf "name=\"new\"\n" > "$0/ci/checks/gameplay/003-new.toml"' "$TREE"
mutate_case "jar" jar \
    bash -c 'printf "a different jar\n" > "$0/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"' "$TREE"
mutate_case "recorder" recorder \
    bash -c 'printf "pub fn extra() {}\n" >> "$0/tools/oracle-runner/src/record.rs"' "$TREE"
mutate_case "harness" harness \
    bash -c 'printf "# a substrate change\n" >> "$0/ci/t2-gameplay.sh"' "$TREE"
mutate_case "tables" tables \
    bash -c 'printf "{\"items\":{\"minecraft:stone\":2}}\n" > "$0/data/vanilla/items.json"' "$TREE"

# The decoder half of the recorder. `recorder` above covers
# tools/oracle-runner/src; this covers the crate that file decodes THROUGH, and
# the two are separate cases because they are separate directories and a fix to
# one has never implied a fix to the other.
mutate_case "proto" proto \
    bash -c 'printf "pub fn decode() { /* field order changed */ }\n" > "$0/crates/rustmc-proto/src/wire.rs"' "$TREE"


# The projection moves through the binary's answer, not through a file, which
# is the point: `ci/t2-gameplay.sh` takes it from the binary it is about to run
# and not from the tree it is standing in.
seed_entry
OUT="$(PROJ=proj:0000000000000000 refcache_lookup "$TREE" "$(BIN)" "$SEED" "$WORK/out.json" 2>&1)"; RC=$?
if [ $RC -ne 0 ] && printf '%s' "$OUT" | grep -c projection >/dev/null; then
    ok "projection — a different projection-id misses, and the miss names \`projection\`"
else bad "projection — rc=$RC out=$(printf '%s' "$OUT" | head -1)"; fi

# The seed is an argument rather than a file, so it gets its own shape.
seed_entry
OUT="$(refcache_lookup "$TREE" "$(BIN)" 12345 "$WORK/out.json" 2>&1)"; RC=$?
[ $RC -ne 0 ] && ok "seed — a different GAMEPLAY_SEED misses" \
              || bad "seed — a different seed SERVED THE SAME REFERENCE"

# --- case 11: the 2026-08-03 instance, reconstructed -------------------------
# A guard that cannot be shown catching a real past defect is itself a check
# that cannot fail. On 2026-08-03 a gate reported 12/55 with 11 regressed for a
# branch that was fine, because `data/vanilla` was shared by PATH between trees
# and one tree's table was another tree's. Reconstructed here: the recording is
# stored against one table set and the tree is then handed a different one, as
# a shared `data/vanilla` would.
# 2026-09-24: a reference recorded in one shard layout must not be served to a
# run in another (`refcache_components`' `shards=` line says why). Stored at the
# default, looked up at 4, and the miss must name the component.
seed_entry
OUT="$(RUSTMC_CORPUS_SHARDS=4 refcache_lookup "$TREE" "$(BIN)" "$SEED" "$WORK/out.json" 2>&1)"; RC=$?
if [ $RC -ne 0 ] && printf '%s' "$OUT" | grep -c shards >/dev/null; then
    ok "shards — a different RUSTMC_CORPUS_SHARDS misses, and the miss names \`shards\`"
else bad "shards — rc=$RC out=$(printf '%s' "$OUT" | head -1)"; fi

seed_entry
cp "$TREE/data/vanilla/items.json" "$WORK/ours.json"
printf '{"items":{"minecraft:stone":1,"minecraft:bed":2}}\n' > "$TREE/data/vanilla/items.json"
OUT="$(try_lookup)"; RC=$?
if [ $RC -ne 0 ] && printf '%s' "$OUT" | grep -c tables >/dev/null; then
    ok "regression-20260803 — another tree's data/vanilla misses, naming \`tables\`"
else bad "regression-20260803 — rc=$RC, the miss did not name \`tables\`: $(printf "%s" "$OUT" | tr "\n" " " | head -c 160)"; fi

# --- cases 12..13: the stored artefact itself --------------------------------
# The key describes the INPUTS. Nothing about it can notice that the OUTPUT was
# damaged after it was written, so that is checked separately and must refuse.
seed_entry
K="$(refcache_key "$TREE" "$(BIN)" "$SEED")"; E="$REFCACHE_DIR/${K#ref:}"
: > "$E/vanilla.json"
OUT="$(try_lookup)"; RC=$?
[ $RC -ne 0 ] && ok "truncated — an emptied entry is refused, not served" \
              || bad "truncated — served an empty recording"

seed_entry
K="$(refcache_key "$TREE" "$(BIN)" "$SEED")"; E="$REFCACHE_DIR/${K#ref:}"
printf '{"target":"127.0.0.1:0","scenarios":[{"name":"TAMPERED"}]}\n' > "$E/vanilla.json"
OUT="$(try_lookup)"; RC=$?
if [ $RC -ne 0 ] && printf '%s' "$OUT" | grep -c 'does not match its own digest' >/dev/null; then
    ok "tampered — a rewritten entry is refused against its stored digest"
else bad "tampered — rc=$RC, a rewritten recording was served"; fi

# --- case 14: a hand-edited component block ----------------------------------
# The key matched but the components disagree. Only reachable through damage or
# a key collision, and it must still refuse rather than trust the hash.
seed_entry
K="$(refcache_key "$TREE" "$(BIN)" "$SEED")"; E="$REFCACHE_DIR/${K#ref:}"
sed -i 's/^jar=.*/jar=deadbeefdeadbeef/' "$E/components"
OUT="$(try_lookup)"; RC=$?
[ $RC -ne 0 ] && ok "component-block — an entry whose components disagree is refused" \
              || bad "component-block — trusted the key over the components"

# --- case 15: a missing input is not a component that matches ----------------
# Two trees that are both missing the jar must not agree with each other. This
# is the empty-string collision that `MISSING` exists to prevent.
seed_entry
rm -f "$TREE/oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar"
OUT="$(try_lookup)"; RC=$?
if [ $RC -ne 0 ] && printf '%s' "$OUT" | grep -c 'jar=MISSING' >/dev/null; then
    ok "missing-input — an absent jar refuses by name rather than hashing to nothing"
else bad "missing-input — rc=$RC out=$(printf '%s' "$OUT" | head -1)"; fi

# --- case 16: the flag actually gates it -------------------------------------
seed_entry
OUT="$(RUSTMC_REFERENCE_CACHE= refcache_lookup "$TREE" "$(BIN)" "$SEED" "$WORK/out.json" 2>&1)"; RC=$?
[ $RC -ne 0 ] && ok "opt-in — with RUSTMC_REFERENCE_CACHE unset nothing is served" \
              || bad "opt-in — served a reference with the cache disabled"

# --- case 17: an existing entry is never rewritten ---------------------------
seed_entry
K="$(refcache_key "$TREE" "$(BIN)" "$SEED")"; E="$REFCACHE_DIR/${K#ref:}"
BEFORE="$(sha256sum "$E/vanilla.json" | cut -d' ' -f1)"
printf '{"target":"x","scenarios":[{"name":"second"}]}\n' > "$TREE/recording2.json"
refcache_store "$TREE" "$(BIN)" "$SEED" "$TREE/recording2.json" >/dev/null 2>&1
AFTER="$(sha256sum "$E/vanilla.json" | cut -d' ' -f1)"
[ "$BEFORE" = "$AFTER" ] && ok "immutable — storing again over one key does not rewrite the entry" \
                         || bad "immutable — a second store overwrote a live entry"

# --- case 18: the key covers every table the loader opens --------------------
# If `client::Tables::load` ever opens a table the key does not cover, that
# file decides what the harness sends TO vanilla while sitting outside the key
# — a reference that silently stops matching its own inputs.
#
# **BOTH sides are derived now, and until 2026-08-18 one of them was not.**
# `WANT` was the literal string `"block_props.json entity_types.json
# items.json "`, so this case compared a set read from the loader against a set
# somebody had typed. That is half a check: it catches the loader growing a
# table, which is what it was written for, and it cannot catch the key and the
# list disagreeing — and a list kept true by remembering is this repository's
# oldest defect. Reading `reference-cache.sh`'s own `for t in ...` loop makes
# both sides facts about the tree.
#
# It caught one the same day it was widened: `corpus-sound-pitch` added
# `protocol_ids.json` to `Tables::load`, because `pitch.rs` resolves a sound
# event id through the registry, and did not add it to the key.
LOADED="$(grep -oE 'dir\.join\("[a-z_]+\.json"\)' "$ROOT/tools/oracle-runner/src/client.rs" \
          | grep -oE '"[a-z_]+\.json"' | tr -d '"' | sort -u | tr '\n' ' ')"
WANT="$(sed -n 's/^ *for t in \(.*\); do$/\1/p' "$ROOT/ci/reference-cache.sh" \
        | tr ' ' '\n' | sed '/^$/d;s/$/.json/' | sort -u | tr '\n' ' ')"
if [ -z "$WANT" ]; then
    bad "tables-set — could not read the key's table list out of ci/reference-cache.sh"
elif [ "$LOADED" = "$WANT" ]; then
    ok "tables-set — Tables::load opens exactly the files the key covers { $WANT}"
else
    bad "tables-set — Tables::load opens { $LOADED}, the key covers { $WANT} — update reference-cache.sh"
fi

# --- case 18b: two different tree roots, identical content, MUST hit --------
# The entire premise of this cache is "sixty trees share one recording", and
# every case above proves half of that: change the content and the key moves.
# Nothing above proves the other half — that the tree's PATH is not in the key
# and a second, differently-located clone with the same content is served the
# first tree's entry rather than treated as a stranger. Without this, a design
# that happens to work because `mktree` always builds at the same `$TREE` path
# would ship unverified: `refcache_key` takes `root` as an argument and never
# hashes it, and nothing until now exercised that by actually using two roots.
TREE2="$WORK/tree2"
mktree "$TREE2"
K1="$(refcache_key "$TREE" "$(BIN)" "$SEED")"
K2="$(refcache_key "$TREE2" "$TREE2/bin/oracle-runner" "$SEED")"
[ "$K1" = "$K2" ] && ok "root-invariance — two different tree paths with identical content compute the same key" \
                  || bad "root-invariance — identical content at a different path moved the key: $K1 -> $K2"
# A SEED THIS CASE OWNS ALONE. `$SEED` (20260803) is what every earlier case
# in this file stores and re-stores against, and `refcache_store` is
# immutable (case "immutable" above) — so cases 12-14 permanently corrupt
# that shared entry's `components`/`vanilla.json` on purpose, as part of
# THEIR OWN assertions, and nothing after them ever re-legitimises it. A hit
# case landing on that key would silently inherit case 14's `jar=deadbeef`
# corruption and refuse for a reason that has nothing to do with roots — found
# by running this exact case against the shared seed first. Isolating the key
# is the fix, not chasing case ordering: the same shape of test-isolation leak
# `docs-ledger-audit` found and fixed in `ci/check-figures-falsify.py` the
# same night this file was last touched.
RISEED=99990018
mktree "$TREE"
refcache_store "$TREE" "$(BIN)" "$RISEED" "$TREE/recording.json" >/dev/null 2>&1
OUT="$(refcache_lookup "$TREE2" "$TREE2/bin/oracle-runner" "$RISEED" "$WORK/out2.json" 2>&1)"; RC=$?
if [ $RC -eq 0 ] && printf '%s' "$OUT" | grep -c 'HIT' >/dev/null; then
    ok "root-invariance — a second, differently-located clone hits the first tree's entry"
else
    bad "root-invariance — a fresh clone with identical content did not hit: rc=$RC $(printf '%s' "$OUT" | head -1)"
fi

# --- case 19: the hazard this cache CREATES, and its guard --------------------
# A cache that serves one recording to N trees makes N byte-identical
# `oracle-work/gameplay/vanilla.json` files, and `ci/corpus-floor.sh`'s own
# usage line globs exactly those. Identical recordings disagree by zero at every
# scenario, so a floor taken over copies reports "the reference reproduced it"
# for the whole corpus — a floor of zero manufactured by the instrument, which
# is the defect class this repository exists to refuse.
#
# The guard is in `ci/corpus-floor.sh` and it is asserted here, beside the
# change that made the hazard real, rather than left to be noticed later.
# It runs before the release build, so this costs nothing.
DUP="$WORK/dup"; mkdir -p "$DUP"
printf '{"recorder":"oracle-runner 0.1.0","scenarios":[{"name":"a"}]}\n' > "$DUP/one.json"
cp "$DUP/one.json" "$DUP/two.json"
OUT="$(cd "$ROOT" && ./ci/corpus-floor.sh --out "$DUP/out" "$DUP/one.json" "$DUP/two.json" 2>&1)"; RC=$?
if [ $RC -ne 0 ] && printf '%s' "$OUT" | grep -c 'byte-identical recordings' >/dev/null; then
    ok "floor-dupes — two copies of one recording are refused as an ensemble"
else
    bad "floor-dupes — rc=$RC, copies were accepted as independent runs: $(printf '%s' "$OUT" | head -2 | tr '\n' ' ')"
fi
# ...and the guard must not fire on genuinely distinct recordings, or it would
# refuse every real ensemble and be removed by the first person it blocked.
printf '{"recorder":"oracle-runner 0.1.0","scenarios":[{"name":"b"}]}\n' > "$DUP/two.json"
OUT="$(cd "$ROOT" && ./ci/corpus-floor.sh --out "$DUP/out" "$DUP/one.json" "$DUP/two.json" 2>&1)"
printf '%s' "$OUT" | grep -c 'byte-identical recordings' >/dev/null \
    && bad "floor-dupes — the guard fired on two DISTINCT recordings" \
    || ok "floor-dupes — distinct recordings pass the guard"

echo
echo "reference-cache-falsify: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] || exit 1
