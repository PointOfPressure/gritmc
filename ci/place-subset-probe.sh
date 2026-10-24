#!/usr/bin/env bash
# place-subset-probe.sh — are 730/731/732 stable against vanilla, before
# spending the box-wide corpus lock on a full run?
#
# # Why this exists rather than `ci/t2-gameplay-selfcheck.sh`
#
# The standing rule is that anything which flaps between two cold vanilla runs
# is written into `ci/checks/gameplay-unstable.toml` with its rate or is not
# written at all, and `ci/t2-gameplay-selfcheck.sh` is the instrument for it —
# 12 cold boots of the **whole** corpus, which at 96 scenarios and ~9 s each is
# some three hours holding a lock five branches and a gate chain are queued on.
# Paying that to learn whether three new files flap is not a trade this project
# can make while the merge chain is live.
#
# So this records **only the three new scenarios**, N times, on N cold-booted
# vanilla servers, and compares every run against the first with the same
# comparator. It is `t2-gameplay-selfcheck.sh`'s question asked of a subset.
#
# WHAT IT DOES NOT ANSWER, and this is not a caveat to skip. A subset run puts
# the three checks at workspaces 0, 1 and 2 instead of 93, 94 and 95, so their
# absolute coordinates differ from a full run's and they inherit none of the
# corpus's accumulated session state — no earlier check's free-slot layout, no
# earlier container's counters. **A pass here is evidence that the mechanism is
# reproducible, not that the check is stable in corpus position.** The full run
# at the end is what baselines them; this only decides whether it is worth
# taking the lock.
#
# # It takes the corpus lock, as role `run`, and the first version did not
#
# **That was wrong and it was measured wrong.** The first run of this script
# overlapped `~/rustmc-menu2`'s live corpus recording, and the AGENT-BRIEF has
# the price: overlapping runs do not contend for CPU, they wait out each
# other's `settle_ms` timers, and throughput fell to about one scenario per ten
# minutes at load average 0.02 the last time three ran at once. The exemption
# list — `ci/gen-vanilla-region.sh`, the benches, `ci/collide-falsify.sh` — is a
# list of things that are **a different resource**. This is not one of them: it
# boots vanilla and drives scenarios through the recorder, which is exactly the
# resource the corpus lock exists to serialise. Being short is not being
# exempt.
#
# So it declares as role `run`, which means a real merge gate still jumps ahead
# of it. **Window: about a minute per run, N runs, so under ten minutes at
# N = 6** — stated here because the standing rule is that anything sitting
# inside the corpus lock says in its header how long it holds it, or the next
# agent reads it as a violation.
#
#     ./ci/place-subset-probe.sh [N]        # N defaults to 6

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
N="${1:-6}"
PORT="${PROBE_PORT:-25777}"
SEED="${GAMEPLAY_SEED:-20260803}"
OUT="${PROBE_OUT:-$ROOT/oracle-work/gameplay/place-probe}"
SUBSET="$OUT/scenarios"

command -v cargo >/dev/null || { echo "cargo not on PATH"; exit 2; }

# Role `run`, never `gate`: this is not a merge gate and must not jump the
# queue ahead of one. Acquired before the first boot and held for the whole
# probe, because releasing between runs would let a full corpus interleave and
# put this script back in the overlap it exists to stay out of.
export RUSTMC_CORPUS_LOCK_ROLE="${RUSTMC_CORPUS_LOCK_ROLE:-run}"
. "$ROOT/ci/corpus-lock.sh"
corpus_lock_acquire

rm -rf "$OUT"; mkdir -p "$SUBSET"
for f in "$ROOT"/ci/checks/gameplay/73?-*.toml; do
    [ -e "$f" ] || { echo "no 73x scenarios found — nothing to probe"; exit 2; }
    cp "$f" "$SUBSET/"
done
echo "probing $(ls "$SUBSET" | wc -l) scenarios, N=$N"

cargo build --release -p oracle-runner >/dev/null 2>&1 || {
    echo "oracle-runner failed to build"; exit 2; }
RUNNER="$ROOT/target/release/oracle-runner"

# THE FOUR GAME RULES ARE NOT OPTIONAL AND ARE NOT COPIED FROM MEMORY.
#
# They are parsed out of `ci/t2-gameplay.sh`'s own `gamerule_or_die` calls, so
# this probe cannot drift from the corpus it is predicting. A probe run with
# random ticks left on is a *different world* from the one `ci/t2-gameplay.sh`
# measures: the grass under every rig starts dying mid-window and emits
# `block_update`s that have nothing to do with the check, which would report
# three perfectly deterministic scenarios as unstable. That failure has already
# been paid for once at the corpus level — 26.2 renamed these ids to snake_case
# and the old camelCase ones were refused in silence for a week — which is why
# the refusal is read here too.
mapfile -t RULES < <(sed -nE 's/^gamerule_or_die "\$VAN_DIR" (\S+) (\S+)\s*$/\1 \2/p' \
    "$ROOT/ci/t2-gameplay.sh")
[ "${#RULES[@]}" -ge 3 ] || { echo "parsed only ${#RULES[@]} gamerules out of ci/t2-gameplay.sh"; exit 2; }
echo "gamerules from ci/t2-gameplay.sh: ${RULES[*]}"

# THE SERVER PROPERTIES ARE THE OTHER HALF, AND LEAVING THEM OUT PRODUCED SIX
# IDENTICAL RECORDINGS OF NOTHING HAPPENING — 2026-08-12.
#
# `ci/t2-gameplay.sh` boots vanilla with `gamemode=creative` among four extra
# properties. `Step::Hold` is `set_creative_mode_slot`, which a **survival**
# server ignores outright, so the actor held nothing, every `place` did
# nothing, and the only `block_update`s in the trace were vanilla correcting
# the client's prediction back to `air`. Run 1 of the first probe recorded
# exactly that: `minecraft:air` at [1,0,1] and [1,1,1] for all three
# scenarios.
#
# **And the probe would have called it STABLE.** Six cold boots of a
# misconfigured world agree with each other perfectly. That is this project's
# rule 7 in its purest form — when a measurement clears every candidate,
# suspect the measurement — so the fix is in two parts, and the second matters
# more than the first: parse the properties from the corpus script (below), and
# refuse a recording in which nothing was placed (`assert_live`, further down).
EXTRA_LINE="$(sed -nE 's/^(VANILLA_EXTRA_PROPS=.*[^\\])\s*\\?\s*$/\1/p' \
    "$ROOT/ci/t2-gameplay.sh" | head -1)"
[ -n "$EXTRA_LINE" ] || { echo "could not parse VANILLA_EXTRA_PROPS out of ci/t2-gameplay.sh"; exit 2; }
eval "export $EXTRA_LINE"
case "$VANILLA_EXTRA_PROPS" in
    *gamemode=creative*) ;;
    *) echo "parsed VANILLA_EXTRA_PROPS has no gamemode=creative — refusing:"
       printf '%s\n' "$VANILLA_EXTRA_PROPS"; exit 2 ;;
esac
echo "server props from ci/t2-gameplay.sh: $(printf '%s' "$VANILLA_EXTRA_PROPS" | tr '\n' ' ')"

for i in $(seq 1 "$N"); do
    VAN="$OUT/van-$i"
    "$ROOT/ci/vanilla-server.sh" start "$VAN" "$PORT" "minecraft:flat" "$SEED" \
        >"$OUT/boot-$i.log" 2>&1 || { echo "run $i: vanilla would not boot"; exit 3; }
    for r in "${RULES[@]}"; do
        "$ROOT/ci/vanilla-server.sh" console "$VAN" "gamerule $r" >/dev/null 2>&1 || {
            echo "run $i: could not deliver 'gamerule $r'"; exit 3; }
    done
    sleep 1
    if grep -qE "Incorrect argument|Unknown or incomplete" "$VAN/logs/latest.log" 2>/dev/null; then
        echo "run $i: vanilla REFUSED a gamerule — see $VAN/logs/latest.log"
        grep -E "Incorrect argument|Unknown or incomplete" -A1 "$VAN/logs/latest.log" | tail -4
        exit 3
    fi
    "$RUNNER" record "127.0.0.1:$PORT" "$SUBSET" --out "$OUT/r$i.json" \
        >"$OUT/rec-$i.log" 2>&1
    rc=$?
    "$ROOT/ci/vanilla-server.sh" stop "$VAN" >/dev/null 2>&1
    [ $rc -eq 0 ] || { echo "run $i: record failed rc=$rc"; tail -5 "$OUT/rec-$i.log"; exit 3; }
    echo "  run $i recorded"
done

# assert_live — did anything this scenario placed ever reach the wire?
#
# Six identical recordings of a world where nothing happened are perfectly
# reproducible, and a stability probe that only compares runs to each other
# calls that a pass. `tools/oracle-runner`'s `vacuous` and `inert` verdicts are
# this same idea applied to a reference; there is no reference here — both
# sides are vanilla — so the subtraction has to be done directly: strip the
# substrate and the empty state, and ask whether anything is left.
#
# The substrate list is vanilla's superflat, which `ci/dump-flat-preset.sh`
# reports as 4 layers. It is written out rather than derived because the point
# of the assertion is to be blunt: any block a scenario places is outside it.
python3 - "$OUT" "$N" <<'PY' || exit 4
import json, os, sys
out, n = sys.argv[1], int(sys.argv[2])
SUBSTRATE = {"minecraft:air", "minecraft:grass_block", "minecraft:dirt", "minecraft:bedrock", ""}
dead = []
for i in range(1, n + 1):
    d = json.load(open(os.path.join(out, f"r{i}.json")))
    for s in d.get("scenarios", []):
        placed = set()
        for o in s.get("obs", ()):
            k, f = o.get("kind"), o.get("fields", {})
            if k == "block_update":
                placed.add(f.get("state", "").split("[")[0])
            elif k == "section_blocks_update":
                for b in f.get("blocks", ()):
                    placed.add(b.get("state", "").split("[")[0])
        if not (placed - SUBSTRATE):
            dead.append((i, s.get("name", "?"), sorted(placed)))
if dead:
    print("NOT LIVE — these scenarios put no non-substrate block on the wire.")
    print("A probe over recordings like these measures the harness, not the mechanism.")
    for i, name, seen in dead[:12]:
        print(f"  r{i}  {name}  saw only {seen}")
    sys.exit(4)
print("live: every scenario in every run put a placed block on the wire")
PY

echo
fail=0
for i in $(seq 2 "$N"); do
    "$RUNNER" compare "$SUBSET" --reference "$OUT/r1.json" --subject "$OUT/r$i.json" \
        --json "$OUT/cmp-$i.json" >"$OUT/cmp-$i.log" 2>&1
    # A vanilla run that disagrees with another vanilla run is an unstable
    # check, not a defect. Anything other than `match` on every scenario here
    # is the answer this script exists to get.
    if grep -qE '\b(diverge|vacuous|inert|error|absent)\b' "$OUT/cmp-$i.log"; then
        echo "UNSTABLE  r1 vs r$i:"
        grep -E '\b(diverge|vacuous|inert|error|absent)\b' "$OUT/cmp-$i.log" | head -6
        fail=1
    else
        echo "  r1 vs r$i  all match"
    fi
done

echo
if [ $fail -eq 0 ]; then
    echo "PROBE STABLE over $N cold vanilla boots — the mechanisms reproduce."
    echo "This is NOT a baseline. Run ci/t2-gameplay.sh for verdicts."
else
    echo "PROBE UNSTABLE — write the rate into ci/checks/gameplay-unstable.toml"
    echo "or delete the check. Do not baseline it."
fi
exit $fail
